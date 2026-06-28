use crate::memory::constants::HEAP_PAGE_COUNT;

#[derive(Debug, Copy, Clone)]
pub struct BuddyAllocator {
    page_used: [bool; HEAP_PAGE_COUNT],
}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            page_used: [false; HEAP_PAGE_COUNT],
        }
    }

    pub fn alloc_pages(&mut self, pages: usize) -> Option<usize> {
        if pages == 0 || pages > HEAP_PAGE_COUNT {
            return None;
        }

        let mut run_start = 0;
        let mut run_length = 0;
        for (index, used) in self.page_used.iter().copied().enumerate() {
            if used {
                run_start = index + 1;
                run_length = 0;
                continue;
            }

            run_length += 1;
            if run_length == pages {
                for page in run_start..(run_start + pages) {
                    self.page_used[page] = true;
                }
                return Some(run_start);
            }
        }

        None
    }

    pub fn free_pages(&mut self, start_page: usize, pages: usize) {
        for page in start_page..(start_page + pages).min(HEAP_PAGE_COUNT) {
            self.page_used[page] = false;
        }
    }
}
