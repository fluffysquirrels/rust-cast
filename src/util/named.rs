#[macro_export]
macro_rules! function_path {
    () => (concat!(
        module_path!(), "::", function_name!()
    ))
}

#[macro_export]
macro_rules! method_path {
    ($struct_name: literal) => (concat!(
        module_path!(), "::",
        $struct_name, "::",
        function_name!()
    ))
}

pub use function_name::named;
