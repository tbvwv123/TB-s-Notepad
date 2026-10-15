//! Windows 실행 파일에 아이콘·파일 정보 리소스(assets/app.rc)를 넣습니다.
//!
//! GNU 툴체인의 windres 를 씁니다. 프로젝트 경로의 작은따옴표(TB's)가 windres 가 부르는
//! 전처리기 인자를 깨뜨리므로, assets 폴더 안에서 상대 경로로만 실행합니다.
//! windres 가 없으면 아이콘 없이 빌드를 계속합니다.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/logo.ico");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    if !target.ends_with("-gnu") && !target.ends_with("-gnullvm") {
        println!("cargo:warning=MSVC 대상은 아직 아이콘 리소스를 넣지 않습니다 (windres 전용)");
        return;
    }

    let assets = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app_res.o");
    let status = Command::new("windres")
        .current_dir(&assets)
        .args(["--input=app.rc", "--output-format=coff", "--codepage=65001"])
        .arg(format!("--output={}", out.display()))
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        Ok(s) => println!("cargo:warning=windres 실패({s}): 실행 파일 아이콘 없이 빌드합니다"),
        Err(e) => println!("cargo:warning=windres 를 찾지 못함({e}): 실행 파일 아이콘 없이 빌드합니다"),
    }
}
