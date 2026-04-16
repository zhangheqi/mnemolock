pub struct Defer<F: FnMut()>(F);

impl<F: FnMut()> Defer<F> {
    pub fn new(func: F) -> Self {
        Self(func)
    }
}

impl<F: FnMut()> Drop for Defer<F> {
    fn drop(&mut self) {
        self.0()
    }
}

#[macro_export]
macro_rules! defer {
    ($target:expr) => {
        let _defer = $crate::util::Defer::new(|| {
            let _ = $target;
        });
    };
}

pub struct BoundedIndex {
    value: usize,
    min: usize,
    max: usize,
}

impl BoundedIndex {
    pub fn new(value: usize, min: usize, max: usize) -> Self {
        Self {
            value,
            min,
            max,
        }
    }

    pub fn increment(&mut self) {
        if self.value < self.max {
            self.value += 1;
        }
    }

    pub fn decrement(&mut self) {
        if self.value > self.min {
            self.value -= 1;
        }
    }

    pub fn value(&self) -> usize {
        self.value
    }
}
