#![feature(new_uninit, allocator_api)]
use backtrace::{BytesOrWideString, Frame, Symbol};
use hashbrown::hash_map::DefaultHashBuilder;
use hashbrown::HashMap;
use heapless::String as HeaplessString;
use heapless::Vec as HeaplessVec;
use once_cell::sync::{Lazy, OnceCell};
use spin::Mutex;
use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Display;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use widestring::U16Str;

#[derive(Debug, Clone)]
pub struct Call {
    pub name: Option<HeaplessString<500>>,
    pub filename: Option<HeaplessString<500>>,
    pub line: Option<u32>,
    pub col: Option<u32>,

    pub addr: usize,
}

impl From<&Symbol> for Call {
    fn from(value: &Symbol) -> Self {
        let addr = value.addr().unwrap() as usize;
        let line = value.lineno();
        let col = value.colno();
        let name = value
            .name()
            .map(|x| HeaplessString::from(x.as_str().unwrap()));
        let filename = value.filename_raw().map(|x| match x {
            BytesOrWideString::Bytes(bytes) => {
                HeaplessString::from(std::str::from_utf8(bytes).unwrap())
            }
            BytesOrWideString::Wide(bytes) => {
                let mut filename = HeaplessString::default();
                for c in U16Str::from_slice(bytes).chars_lossy() {
                    filename.push(c).unwrap();
                }
                filename
            }
        });

        Self {
            name,
            filename,
            line,
            col,
            addr,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AllocationRecord<const STACK_SIZE: usize> {
    pub size: usize,
    pub ptr: usize,
    pub stack: HeaplessVec<Call, STACK_SIZE>,
}

impl<const STACK_SIZE: usize> Display for AllocationRecord<STACK_SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Allocation@{:x} (size {}):\n", self.ptr, self.size)?;
        for s in self.stack.iter() {
            let name = s.name.clone().unwrap_or(HeaplessString::from("[unknown]"));
            let addr = s.addr;
            let filename = s
                .filename
                .clone()
                .unwrap_or(HeaplessString::from("[unknown file]"));
            write!(f, "  {name} {addr:x} @ {filename}")?;
            match (s.line, s.col) {
                (Some(line), Some(col)) => write!(f, ":{line}-{col})")?,
                (Some(line), _) => write!(f, ":{line}")?,
                // What about col present but missing line?
                // Normally this should not happen, so it should be safe to ignore that.
                (_, _) => {}
            };
            write!(f, "\n")?;
        }
        Ok(())
    }
}

pub struct LeakTracerInner<const STACK_SIZE: usize> {
    allocates: Mutex<HashMap<usize, AllocationRecord<STACK_SIZE>, DefaultHashBuilder, System>>,
    enabled: AtomicBool,
}

impl<const STACK_SIZE: usize> Default for LeakTracerInner<STACK_SIZE> {
    fn default() -> Self {
        Self {
            allocates: Mutex::new(HashMap::default()),
            enabled: AtomicBool::new(true),
        }
    }
}

pub struct LeakTracer<const STACK_SIZE: usize>(Lazy<LeakTracerInner<STACK_SIZE>>);

pub type LeakTracerDefault = LeakTracer<10>;

impl<const STACK_SIZE: usize> LeakTracer<STACK_SIZE> {
    pub const fn new() -> Self {
        Self(Lazy::new(|| LeakTracerInner::default()))
    }

    pub fn disable(&self) {
        self.0.enabled.store(false, Ordering::SeqCst);
    }

    pub fn enable(&self) {
        self.0.enabled.store(true, Ordering::SeqCst);
    }

    pub fn get_leaks(
        &self,
    ) -> HashMap<usize, AllocationRecord<STACK_SIZE>, DefaultHashBuilder, System> {
        let cur = self.0.enabled.load(Ordering::SeqCst);
        self.0.enabled.store(false, Ordering::SeqCst);

        let mut out = HashMap::default();
        for (k, v) in self.0.allocates.lock().iter() {
            out.insert(*k, v.clone());
        }

        self.0.enabled.store(cur, Ordering::SeqCst);

        out
    }

    fn alloc_accounting(&self, size: usize, ptr: *mut u8) -> *mut u8 {
        if !self.0.enabled.load(Ordering::SeqCst) {
            return ptr;
        }

        let mut stack = HeaplessVec::default();
        let mut count = 0;
        // First 2 stack is in the closure itself, meaningless, skip that.
        let mut skip_count = 2;
        // On win7 64, it's may cause deadlock, solution is to palce a newer version of dbghelp.dll combined with exe
        unsafe {
            backtrace::trace_unsynchronized(|frame| {
                if skip_count > 0 {
                    skip_count -= 1;
                    return true;
                }

                backtrace::resolve_frame_unsynchronized(frame, |symbol| {
                    stack.push(symbol.into()).unwrap();
                    count += 1;
                });
                if count >= STACK_SIZE {
                    false
                } else {
                    true
                }
            });
        }

        let allocation_record = AllocationRecord {
            size,
            ptr: ptr as usize,
            stack,
        };
        self.0
            .allocates
            .lock()
            .insert(ptr as usize, allocation_record);

        ptr
    }

    fn dealloc_accounting(&self, ptr: *mut u8) {
        if !self.0.enabled.load(Ordering::SeqCst) {
            return;
        }

        self.0.allocates.lock().remove(&(ptr as usize));
    }
}

unsafe impl<const STACK_SIZE: usize> GlobalAlloc for LeakTracer<STACK_SIZE> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc_accounting(layout.size(), System.alloc(layout))
    }

    unsafe fn realloc(&self, ptr0: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = System.realloc(ptr0, layout, new_size);
        if ptr != ptr0 {
            self.dealloc_accounting(ptr0);
            self.alloc_accounting(new_size, ptr);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.dealloc_accounting(ptr);
        System.dealloc(ptr, layout);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let aa = crate::LeakTracer::<15>::new();
    }
}
