//! Builds the bundled libarchive (`vendor/`) as a static library with every
//! optional library off: RAR reading needs none, and nothing is then linked
//! dynamically from the system.
use std::env;

fn main() {
    println!("cargo:rerun-if-changed=vendor");
    let mut config = cmake::Config::new("vendor");
    config
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("ENABLE_TEST", "OFF")
        .define("ENABLE_INSTALL", "OFF")
        .define("ENABLE_WERROR", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
    for option in [
        "ENABLE_ACL",
        "ENABLE_XATTR",
        "ENABLE_ZLIB",
        "ENABLE_BZip2",
        "ENABLE_LZMA",
        "ENABLE_ZSTD",
        "ENABLE_LZ4",
        "ENABLE_LZO",
        "ENABLE_LIBB2",
        "ENABLE_LIBXML2",
        "ENABLE_EXPAT",
        "ENABLE_WIN32_XMLLITE",
        "ENABLE_PCREPOSIX",
        "ENABLE_PCRE2POSIX",
        "ENABLE_LIBGCC",
        "ENABLE_OPENSSL",
        "ENABLE_MBEDTLS",
        "ENABLE_NETTLE",
        "ENABLE_CNG",
        "ENABLE_ICONV",
        "ENABLE_TAR",
        "ENABLE_CPIO",
        "ENABLE_CAT",
        "ENABLE_UNZIP",
    ] {
        config.define(option, "OFF");
    }
    // Windows has no POSIX regex in its C library, and CMake would then
    // look for PCRE (and fail): the regex support is only for `bsdtar`.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        config.define("POSIX_REGEX_LIB", "NONE");
    }
    let out = config.build_target("archive_static").build();
    let build = out.join("build");
    // Makefile and Ninja put the library in `libarchive/`, Visual Studio in
    // a configuration folder below it.
    for sub in ["libarchive", "libarchive/Release", "libarchive/Debug", "libarchive/RelWithDebInfo"]
    {
        println!("cargo:rustc-link-search=native={}", build.join(sub).display());
    }
    println!("cargo:rustc-link-lib=static=archive");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=bcrypt");
    }
}
