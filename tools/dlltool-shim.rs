// dlltool-shim.rs — a minimal dlltool.exe replacement for rustc raw-dylib linking on windows-gnu.
// rustc invokes `dlltool -d <def> -l <out> -m <machine>` (or --input-def/--output-lib) to generate
// an import library. We translate that call into rust-lld's -def/-implib pipeline.
// Place the compiled exe at ~/.cargo/bin/dlltool.exe so cargo build processes find it on PATH.

use std::env;
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // debug logging (opt-in): append argv + def content for diagnosis
    if env::var_os("DLLTOOL_DEBUG").is_some() {
        use std::fs::OpenOptions;
        use std::io::Write;
        let dbg = "%TEMP%\\dlltool-shim.log";
        let dbg = env::var("TEMP").map(|t| format!("{t}\\dlltool-shim.log")).unwrap_or_else(|_| dbg.to_string());
        let mut f = OpenOptions::new().create(true).append(true).open(&dbg).unwrap();
        let _ = writeln!(f, "argv: {}", args.join(" "));
    }

    let mut def: Option<String> = None;
    let mut out: Option<String> = None;
    let mut machine: Option<String> = None;
    let mut dllname: Option<String> = None;
    let mut unknown: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let mut take_val = |flag: &str, store: &mut Option<String>| -> bool {
            if a == flag {
                if i + 1 < args.len() {
                    *store = Some(args[i + 1].clone());
                    i += 1;
                    return true;
                }
            }
            false
        };
        let mut take_val_eq = |flag: &str, store: &mut Option<String>| -> bool {
            if let Some(v) = a.strip_prefix(flag) {
                if !v.is_empty() {
                    *store = Some(v.to_string());
                    return true;
                }
            }
            false
        };
        if take_val("-d", &mut def) || take_val("--input-def", &mut def) || take_val("--def", &mut def)
            || take_val("-l", &mut out) || take_val("--output-lib", &mut out) || take_val("-o", &mut out)
            || take_val("-m", &mut machine) || take_val("-M", &mut machine)
            || take_val("-D", &mut dllname) || take_val("--dll-name", &mut dllname)
            || take_val_eq("--input-def=", &mut def) || take_val_eq("--output-lib=", &mut out)
            || take_val_eq("-d=", &mut def) || take_val_eq("-l=", &mut out)
            || take_val_eq("-m=", &mut machine) || take_val_eq("-D=", &mut dllname) {
            i += 1;
            continue;
        }
        unknown.push(a.clone());
        i += 1;
    }

    let def = def.unwrap_or_else(|| die("no -d/--input-def argument"));
    let out = out.unwrap_or_else(|| die("no -l/--output-lib argument"));
    let machine = machine.unwrap_or_else(|| "x64".to_string());
    let machine_arg = match machine.as_str() {
        "i386:x86-64" | "x86-64" | "x64" | "i386pep" => "x64",
        _ => {
            eprintln!("dlltool-shim: unsupported machine '{machine}', assuming x64");
            "x64"
        }
    };

    // rustc's def may lack a LIBRARY line; without it lld derives the DLL name from
    // the stub output name, which breaks the final import entries. Prepend LIBRARY
    // from the -D flag so imports reference the real system DLL.
    let def_arg;
    if let Some(dll) = dllname {
        let orig = std::fs::read_to_string(&def).unwrap_or_default();
        let cleaned = orig.lines().filter(|l| !l.trim_start().starts_with("LIBRARY")).collect::<Vec<_>>().join("\r\n");
        let patched = format!("LIBRARY \"{dll}\"\r\nEXPORTS\r\n{cleaned}");
        let patched_path = format!("{def}.qpp");
        if std::fs::write(&patched_path, patched).is_ok() {
            def_arg = patched_path;
        } else {
            def_arg = def.clone();
        }
    } else {
        def_arg = def.clone();
    }

    // rust-lld: create an import library from the .def
    let lld = env::var("DLLTOOL_LLD").unwrap_or_else(|_| {
        "C:\\Users\\550414\\.rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\lib\\rustlib\\x86_64-pc-windows-msvc\\bin\\rust-lld.exe".to_string()
    });
    let base = if out.ends_with(".a") {
        out[..out.len() - 2].to_string()
    } else if out.ends_with(".lib") {
        out[..out.len() - 4].to_string()
    } else {
        out.clone()
    };
    let stub = format!("{base}.stub.dll");
    let def_a = format!("-def:{def_arg}");
    let implib = format!("-implib:{out}");
    let out_arg = format!("-out:{stub}");
    let machine_arg = format!("-machine:{machine_arg}");
    let status = Command::new(&lld)
        .args(["-flavor", "link", &def_a, "-dll", "-noentry", &implib, &out_arg, &machine_arg])
        .status();
    if def_arg != def {
        let _ = std::fs::remove_file(&def_arg);
    }
    match status {
        Ok(s) if s.success() => {
            let _ = std::fs::remove_file(&stub);
            std::process::exit(0);
        }
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => die(&format!("failed to run rust-lld: {e}")),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("dlltool-shim: {msg}");
    std::process::exit(1);
}