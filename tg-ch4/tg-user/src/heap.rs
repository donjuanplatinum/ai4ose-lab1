use core::alloc::GlobalAlloc;

struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl BumpAllocator {
    const fn new() -> Self {
        Self { heap_start: 0, heap_end: 0, next: 0 }
    }
    fn init(&mut self, start: usize, end: usize) {
        self.heap_start = start;
        self.heap_end = end;
        self.next = start;
    }
    fn alloc(&mut self, layout: core::alloc::Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let start = (self.next + align - 1) & !(align - 1);
        if start + size <= self.heap_end {
            self.next = start + size;
            start as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }
}

static mut HEAP: BumpAllocator = BumpAllocator::new();

pub fn init() {
    use crate::sbrk;
    const MEMORY_SIZE: usize = 2 << 20;
    let heap_start = sbrk(0);
    if heap_start != -1 {
        if sbrk(MEMORY_SIZE as i32) != -1 {
            unsafe {
                HEAP.init(heap_start as usize, heap_start as usize + MEMORY_SIZE);
            }
        }
    }
}

struct GlobalAllocatorImpl;

#[global_allocator]
static ALLOCATOR: GlobalAllocatorImpl = GlobalAllocatorImpl;

unsafe impl GlobalAlloc for GlobalAllocatorImpl {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let ptr = unsafe { HEAP.alloc(layout) };
        if ptr.is_null() {
            panic!("User allocation error: {:?}", layout);
        }
        ptr
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}
