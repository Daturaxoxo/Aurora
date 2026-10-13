const EXTENSIONS: [&str; 3] = ["pak", "utoc", "ucas"];

pub fn file_names(base_name: &str) -> impl Iterator<Item = String> + '_ {
    EXTENSIONS
        .iter()
        .map(move |extension| format!("{base_name}.{extension}"))
}
