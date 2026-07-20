fn main() {
    let source_directory = std::path::Path::new("src");
    let parser = source_directory.join("parser.c");
    cc::Build::new()
        .std("c11")
        .include(source_directory)
        .file(&parser)
        .compile("tree-sitter-robine");
    println!("cargo:rerun-if-changed={}", parser.display());
}
