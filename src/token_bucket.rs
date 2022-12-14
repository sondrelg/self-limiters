use std::time::Duration;

use bb8_redis::bb8::Pool;
use bb8_redis::RedisConnectionManager;
use log::debug;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;
use pyo3::{PyAny, PyResult, Python};
use pyo3_asyncio::tokio::future_into_py;
use redis::Script;

use crate::errors::SLError;
use crate::generated::TOKEN_BUCKET_SCRIPT;
use crate::utils::{create_connection_manager, create_connection_pool, now_millis, SLResult, REDIS_KEY_PREFIX};

struct ThreadState {
    capacity: u32,
    frequency: f32,
    amount: u32,
    max_sleep: f32,
    connection_pool: Pool<RedisConnectionManager>,
    name: String,
}

impl ThreadState {
    fn from(slf: &TokenBucket) -> Self {
        Self {
            capacity: slf.capacity,
            frequency: slf.refill_frequency,
            amount: slf.refill_amount,
            max_sleep: slf.max_sleep,
            connection_pool: slf.connection_pool.clone(),
            name: slf.name.clone(),
        }
    }
}

async fn schedule_and_sleep(ts: ThreadState) -> SLResult<()> {
    // Connect to redis
    let mut connection = ts.connection_pool.get().await?;

    // Retrieve slot
    let slot: u64 = Script::new(TOKEN_BUCKET_SCRIPT)
        .key(&ts.name)
        .arg(ts.capacity)
        .arg(ts.frequency * 1000.0) // in ms
        .arg(ts.amount)
        .invoke_async(&mut *connection)
        .await?;

    let now = now_millis()?;
    let sleep_duration = {
        // This might happen at very low refill frequencies.
        // Current handling isn't robust enough to ensure
        // exactly uniform traffic when this happens. Might be
        // something worth looking at more in the future, if needed.
        if slot <= now {
            Duration::from_millis(0)
        } else {
            Duration::from_millis(slot - now)
        }
    };

    if ts.max_sleep > 0.0 && sleep_duration > Duration::from_secs_f32(ts.max_sleep) {
        return Err(SLError::MaxSleepExceeded(format!(
            "Received wake up time in {} seconds, which is \
            greater or equal to the specified max sleep of {} seconds",
            sleep_duration.as_secs(),
            ts.max_sleep
        )));
    }

    debug!("Retrieved slot. Sleeping for {}.", sleep_duration.as_secs_f32());
    tokio::time::sleep(sleep_duration).await;

    Ok(())
}

/// Async context manager useful for controlling client traffic
/// in situations where you need to limit traffic to `n` requests per `m` unit of time.
/// For example, when you can only send 1 request per minute.
#[pyclass(frozen)]
#[pyo3(name = "TokenBucket")]
#[pyo3(module = "self_limiters")]
pub(crate) struct TokenBucket {
    #[pyo3(get)]
    capacity: u32,
    #[pyo3(get)]
    refill_frequency: f32,
    #[pyo3(get)]
    refill_amount: u32,
    #[pyo3(get)]
    name: String,
    max_sleep: f32,
    connection_pool: Pool<RedisConnectionManager>,
}

#[pymethods]
impl TokenBucket {
    /// Create a new class instance.
    #[new]
    fn new(
        name: String,
        capacity: u32,
        refill_frequency: f32,
        refill_amount: u32,
        redis_url: Option<&str>,
        max_sleep: Option<f32>,
        connection_pool_size: Option<u32>,
    ) -> PyResult<Self> {
        debug!("Creating new TokenBucket instance");

        if refill_frequency <= 0.0 {
            return Err(PyValueError::new_err("Refill frequency must be greater than 0"));
        }
        // Create redis connection manager
        let manager = create_connection_manager(redis_url)?;

        // Create connection pool
        let pool = create_connection_pool(manager, connection_pool_size.unwrap_or(30))?;

        Ok(Self {
            capacity,
            refill_amount,
            refill_frequency,
            max_sleep: max_sleep.unwrap_or(0.0),
            name: format!("{}{}", REDIS_KEY_PREFIX, name),
            connection_pool: pool,
        })
    }

    /// Spawn a scheduler thread to schedule wake-up times for nodes,
    /// and let the main thread wait for assignment of wake-up time
    /// then sleep until ready.
    fn __aenter__<'p>(&self, py: Python<'p>) -> PyResult<&'p PyAny> {
        let ts = ThreadState::from(self);
        future_into_py(py, async { Ok(schedule_and_sleep(ts).await?) })
    }

    /// Do nothing on aexit.
    #[args(_a = "*")]
    fn __aexit__<'p>(&self, py: Python<'p>, _a: &'p PyTuple) -> PyResult<&'p PyAny> {
        future_into_py(py, async { Ok(()) })
    }

    fn __repr__(&self) -> String {
        format!("Token bucket instance for queue {}", &self.name)
    }
}
