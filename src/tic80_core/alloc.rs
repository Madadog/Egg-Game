// Neither of the original files are commented with licenses,
// So here is the ISC license of the WASM4 project (initial
// origin of this file) followed by the MIT license of the
// TIC-80 project (which modified the original file).

// Copyright (c) Bruno Garcia
// 
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
// 
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
// REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
// FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
// INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
// LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
// OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
// PERFORMANCE OF THIS SOFTWARE.

// MIT License

// Copyright (c) 2017 Vadim Grigoruk @nesbox // grigoruk@gmail.com

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

#![cfg(feature = "buddy-alloc")]

use std::alloc::{GlobalAlloc, Layout};
use std::cell::RefCell;

use buddy_alloc::{BuddyAllocParam, FastAllocParam, NonThreadsafeAlloc};

extern "C" {
    static __heap_base: u8;
}

const FAST_HEAP_SIZE: usize = 24 * 1024;
const LEAF_SIZE: usize = 16;

// This allocator implementation will, on first use, prepare a NonThreadsafeAlloc using all the remaining memory.
// This is done at runtime as a workaround for the fact that __heap_base can't be accessed at compile time.
struct TicAlloc(RefCell<Option<NonThreadsafeAlloc>>);

unsafe impl GlobalAlloc for TicAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let is_none = self.0.borrow().is_none();
        if is_none {
            let mut inner = self.0.borrow_mut();
            *inner = {
                let fast_heap_ptr = std::ptr::addr_of!(__heap_base);
                let heap_ptr = fast_heap_ptr.add(FAST_HEAP_SIZE);
                let heap_size = 0x40000 - (heap_ptr as usize);

                let fast_param = FastAllocParam::new(fast_heap_ptr, FAST_HEAP_SIZE);
                let buddy_param = BuddyAllocParam::new(heap_ptr, heap_size, LEAF_SIZE);
                Some(NonThreadsafeAlloc::new(fast_param, buddy_param))
            };
        }
        self.0.borrow().as_ref().unwrap().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.borrow().as_ref().unwrap().dealloc(ptr, layout)
    }
}

unsafe impl Sync for TicAlloc {}

#[global_allocator]
static ALLOC: TicAlloc = TicAlloc(RefCell::new(None));
