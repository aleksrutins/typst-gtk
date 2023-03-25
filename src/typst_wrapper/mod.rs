mod world;

use std::{path::{PathBuf, Path}, fs};

use world::SystemWorld;
use typst::World;

pub fn compile(filename: String) -> Vec<u8> {
    let mut sysworld = SystemWorld::new(PathBuf::new());
    sysworld.reset();
    sysworld.main = sysworld.resolve(Path::new(&filename)).unwrap();

    let document = typst::compile(&sysworld).unwrap();
    typst::export::pdf(&document)
}

pub fn compile_str(code: String) -> Vec<u8> {
    fs::write("/tmp/typst-live.typ", &code).unwrap();
    compile("/tmp/typst-live.typ".into())
}
