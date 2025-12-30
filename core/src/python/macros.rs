//! Macros for reducing PyO3 boilerplate in enum wrappers

/// Implements common From traits for PyO3 wrapper types
macro_rules! impl_py_from {
    ($py_type:ty, $inner_type:ty) => {
        impl From<$inner_type> for $py_type {
            fn from(inner: $inner_type) -> Self {
                Self { inner }
            }
        }

        impl From<$py_type> for $inner_type {
            fn from(py: $py_type) -> Self {
                py.inner
            }
        }
    };
}

pub(crate) use impl_py_from;
