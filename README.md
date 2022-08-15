# Timely

> This is currently a work in progress.

Timely provides a way to rate-limit your Python code.

The package contains an implementation for concurrency-based time limits
([semaphore](https://en.wikipedia.org/wiki/Semaphore_(programming))),
and an implementation for time-based rate limits
([token bucket](https://en.wikipedia.org/wiki/Token_bucket)).

Both rate-limiters are:

- Async
- Distributed (using Redis)
- Fair (FIFO)
- Performant

The package is written in Rust for improved parallelism.

## Installation

```bash
pip install timely
```

## The semaphore implementation

The semaphore implementation is a concurrency based rate limiter.
It's useful, e.g., to make sure there only `n`active requests to a
restricted resource at the same time.

The flow goes roughly like this:

<img width=800 heigh=800 src="docs/semaphore_aenter.png"></img>

<img width=800 heigh=800 src="docs/semaphore_aexit.png"></img>

It is implemented as a context manager in Python and can be used as follows:

```python
from timely import RedisSemaphore

async with RedisSemaphore(
    name="my-api-queue",  # unique name for the resource we're limiting
    capacity=10,  # 10 concurrent requests are allowed
    redis_url="redis://localhost:6379"
):
    # Perform the rate-limited work immediately
    ...
```

## The token bucket implementation

The token bucket algorithm provides time-based rate limiting. By implementing
it in Rust, we're able to get around the [GIL](https://realpython.com/python-gil/) and
spawn an entirely separate process/thread to perform the work needed to assign tokens
to queued nodes (see [pyo3's](https://pyo3.rs/) section on [parallelism](https://pyo3.rs/v0.16.4/parallelism.html)
for more details).

The code flow is as follows:

<img width=800 heigh=800 src="docs/token_bucket.png"></img>

It is implemented as a context manager in Python and can be used roughly as follows:

```python
from timely import RedisTokenBucket

# Instantiate a bucket that will allow 10 requests per minute
rate_limited_queue = RedisTokenBucket(
    capacity=10,
    refill_frequency=60,
    refill_amount=10,
    redis_url="redis://localhost:6379"
)

while True:
    async with rate_limited_queue:
        # Perform the rate-limited work immediately
        ...
```

## todos

- [ ] Finish first draft of token bucket implementation
- [ ] Create [benchmarks](https://doc.rust-lang.org/cargo/commands/cargo-bench.html)
- [ ] Add python tests
- [ ] Set up build and publish pipeline
