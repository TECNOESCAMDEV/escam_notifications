use std::fs;
use std::path::Path;

fn main() {
    let out_dir = Path::new("static");
    let dist_dir = Path::new("../frontend/dist");

    if dist_dir.exists() {
        let _ = fs::remove_dir_all(out_dir);
        fs::create_dir_all(out_dir).unwrap();
        fs_extra::dir::copy(
            dist_dir,
            out_dir,
            &fs_extra::dir::CopyOptions::new().overwrite(true).copy_inside(true),
        )
            .unwrap();
    }
    println!("cargo:rerun-if-changed=../frontend/dist");
}

