## Idea

It's hard to detect memory leak, with a global allocator, we can trace the `alloc` add `dealloc`, if we record the call stacks of `alloc` operation, then we can see where the code lead memory leak. This tool do NOT record ALL allocation, but delete the record when `dealloc`.

Powerd by `global allocator` + `heapless` + `backtrace`, it's only support nightly toolchain, caused by `new_uninit` features.

## Usage

Add this to your cargo.toml:
```toml
leak-detect-allocator = {git = "https://github.com/Escapingbug/leak-detect-allocator.git"}
```
Example:
```rust
use leak_detect_allocator::{LeakTracerDefault, AllocationRecord};

#[global_allocator]
static LEAK_TRACER: LeakTracerDefault = LeakTracerDefault::new();

#[tokio::main]
async fn main() -> Result<(), BoxError> {

	// .. do some allocations here

	let leaks: HashMap<usize, AllocationRecord, _, _> = LEAK_TRACER.get_leaks();
	// Now we can play with leaks, record or print or whatever.

	// The `AllocationRecord` implements `Display` and `Debug`.
	// You can inspect as you want.

	// You can also manually enable or disable it. By default, it is enabled.
	LEAK_TRACER.disable();
	LEAK_TRACER.enable();
}
```

## Customize

If you want more stack traces, set like this:

```rust
use leak_detect_allocator::LeakTracer;

#[global_allocator]
static LEAK_TRACER: LeakTracer<20> = LeakTracer::<20>::new();
```

By default 10 call records are recorded.

## Known Issues
On Win7 64, if you encounter deadlock, you can try place a newer version of dbghelp.dll to your bin directory.