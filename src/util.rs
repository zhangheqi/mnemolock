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
