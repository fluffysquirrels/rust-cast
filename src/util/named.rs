macro_rules! function_path {
    () => (concat!(
        module_path!(), "::", function_name!()
    ))
}

pub use function_name::named;
