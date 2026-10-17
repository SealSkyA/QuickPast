// windres-shim.rs — a minimal windres.exe replacement for tauri-winres/embed-resource on windows-gnu.
// embed-resource invokes: windres --input <rc> --output <out> --include-dir <dir> --output-format=coff
// It emits cargo:rustc-link-arg-bins=<out>, so rustc passes <out> straight onto the lld link line.
// We therefore emit a real COFF object containing a .rsrc section (ICON + inline manifest;
// VERSIONINFO skipped). Directory offsets are BAKED section-relative offsets with bit 31 set
// (PE resource trees are relative to the base of the .rsrc section), while every resource DATA
// ENTRY offset is emitted as an ADDR32NB relocation against the .rsrc section symbol — PE data
// entry offsets are image-base-relative RVAs, so lld must relocate them. Without those
// relocations Windows reads data from the wrong address and rejects the image at load.
// Place the compiled exe at ~/.cargo/bin/windres.exe.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if env::var_os("WINDRES_DEBUG").is_some() {
        let tmp = env::var("TEMP").unwrap_or_else(|_| ".".into());
        let mut f = fs::OpenOptions::new().create(true).append(true).open(format!("{tmp}\\windres-shim.log")).unwrap();
        use std::io::Write;
        let _ = writeln!(f, "argv: {}", args.join(" "));
    }

    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if (a == "--input" || a == "-i") && i + 1 < args.len() { input = Some(args[i + 1].clone()); i += 1; }
        else if (a == "--output" || a == "-o" || a == "-fo") && i + 1 < args.len() { output = Some(args[i + 1].clone()); i += 1; }
        else if let Some(v) = a.strip_prefix("--input=") { input = Some(v.to_string()); }
        else if let Some(v) = a.strip_prefix("--output=") { output = Some(v.to_string()); }
        i += 1;
    }

    let rc_path = input.unwrap_or_else(|| die("no --input argument"));
    let out_path = output.unwrap_or_else(|| die("no --output argument"));

    // Diagnostic mode: emit a bare, valid COFF object (no sections) so we can
    // bisect whether lld crashes on the *presence* of the resource object or
    // on the *content* of the .rsrc section.
    if env::var_os("WINDRES_EMPTY_OBJ").is_some() {
        let mut obj: Vec<u8> = Vec::new();
        obj.extend_from_slice(&0x8664u16.to_le_bytes()); // machine x64
        obj.extend_from_slice(&0u16.to_le_bytes());      // zero sections
        obj.extend_from_slice(&0u32.to_le_bytes());      // timestamp
        obj.extend_from_slice(&20u32.to_le_bytes());     // ptr to symbols (empty table)
        obj.extend_from_slice(&0u32.to_le_bytes());      // zero symbols
        obj.extend_from_slice(&0u16.to_le_bytes());      // no optional header
        obj.extend_from_slice(&0u16.to_le_bytes());      // characteristics
        obj.extend_from_slice(&0u32.to_le_bytes());      // COFF string table size (0)
        fs::write(&out_path, &obj).unwrap_or_else(|e| die(&format!("cannot write {out_path}: {e}")));
        std::process::exit(0);
    }

    let rc_text = fs::read_to_string(&rc_path).unwrap_or_else(|e| die(&format!("cannot read {rc_path}: {e}")));

    // ---- parse .rc ----
    let mut icon_group_id: u16 = 0;
    let mut icon_path: Option<String> = None;
    let mut manifest: Option<String> = None;

    let lines: Vec<String> = rc_text.lines().map(|l| l.to_string()).collect();
    let mut idx = 0;
    let mut in_version = false;
    let mut vdepth = 0;
    while idx < lines.len() {
        let line = lines[idx].trim_end();
        if in_version {
            if line.contains('{') { vdepth += 1; }
            if line.contains('}') {
                vdepth -= 1;
                if vdepth <= 0 { in_version = false; }
            }
            idx += 1;
            continue;
        }
        if line.contains("VERSIONINFO") {
            in_version = true;
            vdepth = 0;
            idx += 1;
            continue;
        }
        // ICON line: "<id> ICON "<path>""
        if let Some(cap) = line.find("ICON") {
            let head = &line[..cap];
            let id: u16 = head.trim().parse().unwrap_or(0);
            if let Some(q) = line.find('"') {
                let rest = &line[q + 1..];
                if let Some(q2) = rest.find('"') {
                    icon_group_id = id;
                    icon_path = Some(rest[..q2].replace("\\\\", "\\"));
                    idx += 1;
                    continue;
                }
            }
        }
        // manifest: "<id> 24" followed by an inline { ... } block, or "<id> 24 "<file>""
        if let Some(pos) = line.find(" 24") {
            let head = line[..pos].trim();
            if head.parse::<u16>().is_ok() && line[..pos].trim().parse::<u16>().unwrap() > 0 {
                // maybe inline file path on same line
                let after = line[pos + 3..].trim();
                if let Some(rest) = after.strip_prefix('"') {
                    if let Some(end) = rest.find('"') {
                        let p = rest[..end].replace("\\\\", "\\");
                        if let Ok(s) = fs::read_to_string(&p) { manifest = Some(s); }
                        idx += 1;
                        continue;
                    }
                }
                // inline block
                let mut buf = String::new();
                idx += 1;
                while idx < lines.len() {
                    let l = lines[idx].trim();
                    if l == "}" { break; }
                    let inner = l.trim();
                    if let Some(s) = inner.strip_prefix('"') {
                        if let Some(end) = s.rfind('"') {
                            buf.push_str(&s[..end].replace("\"\"", "\""));
                        }
                    }
                    idx += 1;
                }
                manifest = Some(buf);
                idx += 1;
                continue;
            }
        }
        idx += 1;
    }

    let ico_path = icon_path.unwrap_or_else(|| die("no ICON resource in .rc"));
    let manifest_xml = manifest.unwrap_or_default();

    // Diagnostic: WINDRES_ONLY=manifest -> emit only the manifest type (24);
    // WINDRES_ONLY=icon -> emit only icon types (3, 14); else full tree.
    let only: String = env::var("WINDRES_ONLY").unwrap_or_default();

    // ---- parse .ico PNG entries ----
    let ico = fs::read(&ico_path).unwrap_or_else(|e| die(&format!("cannot read {ico_path}: {e}")));
    if ico.len() < 6 { die("bad ico header"); }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    if ico.len() < 6 + 16 * count { die("bad ico dir"); }
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    let mut grp_entries: Vec<(u8, u8, u16, u16, u32)> = Vec::new(); // w,h,planes,bitcount,size
    for e in 0..count {
        let off = 6 + 16 * e;
        let w = ico[off];
        let h = ico[off + 1];
        let planes = u16::from_le_bytes([ico[off + 4], ico[off + 5]]);
        let bpp = u16::from_le_bytes([ico[off + 6], ico[off + 7]]);
        let size = u32::from_le_bytes([ico[off + 8], ico[off + 9], ico[off + 10], ico[off + 11]]) as usize;
        let data_off = u32::from_le_bytes([ico[off + 12], ico[off + 13], ico[off + 14], ico[off + 15]]) as usize;
        if data_off + size > ico.len() { die("bad ico payload offset"); }
        blobs.push(ico[data_off..data_off + size].to_vec());
        grp_entries.push((w, h, planes, bpp, size as u32));
    }
    if blobs.is_empty() { die("ico has no images"); }
    let n = blobs.len();

    // ---- build .rsrc in-memory layout (PE resource tree, section-relative) ----
    // PE resource trees are 4 levels: root -> type -> name -> LANGUAGE -> data
    // entry. Windows walks the full depth when it looks up the process manifest
    // at load time, so every resource needs its language directory. Offsets are
    // relative to the base of the .rsrc section; bit 31 means "subdirectory".
    // We bake these offsets directly — no COFF relocations are needed.
    let with_icon = only != "manifest";
    let with_manifest = only != "icon";
    let n_root = if with_icon && with_manifest { 3 } else if with_icon { 2 } else { 1 };
    let lang = 0x0409u32;

    // ---- pass 1: compute offsets ----
    let root_hdr = 16u32;
    let root_ents = (n_root as u32) * 8;
    let d3_hdr = if with_icon { 16u32 + (n as u32) * 8 } else { 0 };      // icon type dir
    let d3_lang = if with_icon { (n as u32) * (16 + 8) } else { 0 };       // one lang dir per icon
    let d14_hdr = if with_icon { 16u32 + 8 } else { 0 };                   // group type dir
    let d14_lang = if with_icon { 16u32 + 8 } else { 0 };                  // group lang dir
    let d24_hdr = if with_manifest { 16u32 + 8 } else { 0 };               // manifest type dir
    let d24_lang = if with_manifest { 16u32 + 8 } else { 0 };              // manifest lang dir
    let data_entries_start = root_hdr + root_ents + d3_hdr + d3_lang + d14_hdr + d14_lang + d24_hdr + d24_lang;
    let data_entries_count = (if with_icon { n as u32 + 1 } else { 0 }) + (if with_manifest { 1 } else { 0 });
    let data_entries_size = data_entries_count * 16;
    let blobs_start = data_entries_start + data_entries_size;

    let group_size = 6 + 14 * (n as u32);
    let mut blob_cursor = blobs_start;
    let mut group_off = 0u32;
    if with_icon {
        group_off = blob_cursor;
        blob_cursor += group_size;
        // keep 4-byte alignment for the icon blobs that follow the group blob
        blob_cursor = (blob_cursor + 3) & !3;
    }
    let mut icon_offs: Vec<u32> = Vec::new();
    if with_icon {
        for b in &blobs {
            icon_offs.push(blob_cursor);
            blob_cursor += b.len() as u32;
        }
    }
    let mut manifest_off = if with_manifest { blob_cursor } else { 0 };

    // sub directory offsets (for root entries)
    let mut subd = root_hdr + root_ents;
    let d3_off = if with_icon { subd } else { 0 }; subd += d3_hdr + d3_lang;
    let d14_off = if with_icon { subd } else { 0 }; subd += d14_hdr + d14_lang;
    let d24_off = if with_manifest { subd } else { 0 };

    // lang dir offsets inside each type dir
    let mut d3_lang_offs: Vec<u32> = Vec::new();
    if with_icon {
        let mut p = d3_off + d3_hdr;
        for _ in 0..n { d3_lang_offs.push(p); p += 24; }
    }
    let d14_lang_off = if with_icon { d14_off + d14_hdr } else { 0 };
    let d24_lang_off = if with_manifest { d24_off + d24_hdr } else { 0 };

    // ---- pass 2: emit ----
    let mut data: Vec<u8> = Vec::new();

    fn wr(d: &mut Vec<u8>, v: u32) { d.extend_from_slice(&v.to_le_bytes()); }
    fn wr16(d: &mut Vec<u8>, v: u16) { d.extend_from_slice(&v.to_le_bytes()); }

    // root dir header
    wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, n_root as u16);
    if with_icon {
        wr(&mut data, 3); wr(&mut data, 0x8000_0000 | d3_off);
        wr(&mut data, 14); wr(&mut data, 0x8000_0000 | d14_off);
    }
    if with_manifest {
        wr(&mut data, 24); wr(&mut data, 0x8000_0000 | d24_off);
    }
    // IMPORTANT: the emit order below MUST match the offset math in pass 1
    // exactly, or every baked offset is garbage and FindResourceExW /
    // the SxS manifest lookup fail (ERROR_RESOURCE_NAME_NOT_FOUND).
    // Correct order: root -> type 3 dir -> icon lang dirs -> type 14 dir
    // -> group lang dir -> type 24 dir -> manifest lang dir -> data entries.
    // type 3 dir: icons ids 2..n+1 -> lang dirs
    if with_icon {
        wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, n as u16);
        for i in 0..n {
            wr(&mut data, 2 + i as u32);
            wr(&mut data, 0x8000_0000 | d3_lang_offs[i]);
        }
        // icon lang dirs (one entry each -> data entry, no flag)
        for i in 0..n {
            wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 1);
            wr(&mut data, lang);
            wr(&mut data, data_entries_start + (i as u32) * 16);
        }
        // type 14 dir: group id -> group lang dir
        wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 1);
        wr(&mut data, icon_group_id as u32);
        wr(&mut data, 0x8000_0000 | d14_lang_off);
        // group lang dir -> group data entry (icons 0..n-1, group at n)
        wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 1);
        wr(&mut data, lang);
        wr(&mut data, data_entries_start + (n as u32) * 16);
    }
    // type 24 dir: manifest id 1 -> manifest lang dir
    if with_manifest {
        wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 1);
        wr(&mut data, 1);
        wr(&mut data, 0x8000_0000 | d24_lang_off);
        // manifest lang dir -> manifest data entry. With icons the data
        // entries are icons(0..n-1), group(n), manifest(n+1); without
        // icons the manifest is the only entry (offset 0).
        wr(&mut data, 0); wr(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 0); wr16(&mut data, 1);
        wr(&mut data, lang);
        wr(&mut data, data_entries_start + (if with_icon { (n as u32) + 1 } else { 0 }) * 16);
    }
    let manifest_blob_len: u32 = if with_manifest {
        const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
        XML_DECL.len() as u32 + manifest_xml.split_whitespace().collect::<Vec<_>>().join(" ").len() as u32
    } else { 0 };
    // data entries. Each OffsetToData is an IMAGE-BASE-RELATIVE RVA in the
    // final image (the PE resource data-entry offset is NOT section-relative
    // like the directory offsets). We bake the section-relative offset as the
    // ADDEND and emit one ADDR32NB relocation per field; lld adds the .rsrc
    // section RVA at link time. Record the section-relative position of every
    // offset field so the wrapper below can emit those relocations.
    let mut data_entry_off_positions: Vec<u32> = Vec::new();
    let mut de_idx: u32 = 0;
    if with_icon {
        for i in 0..n {
            data_entry_off_positions.push(data_entries_start + de_idx * 16);
            wr(&mut data, icon_offs[i]);
            wr(&mut data, blobs[i].len() as u32);
            wr(&mut data, 0); wr(&mut data, 0);
            de_idx += 1;
        }
        // group data entry
        data_entry_off_positions.push(data_entries_start + de_idx * 16);
        wr(&mut data, group_off);
        wr(&mut data, group_size);
        wr(&mut data, 0); wr(&mut data, 0);
        de_idx += 1;
    }
    if with_manifest {
        // manifest data entry
        data_entry_off_positions.push(data_entries_start + de_idx * 16);
        wr(&mut data, manifest_off);
        wr(&mut data, manifest_blob_len);
        wr(&mut data, 0); wr(&mut data, 0);
        de_idx += 1;
    }
    // group blob (pad to 4-byte alignment so following blobs stay aligned)
    if with_icon {
        wr16(&mut data, 0); wr16(&mut data, 1); wr16(&mut data, n as u16);
        for e in 0..n {
            let (w, h, planes, bpp, size) = grp_entries[e];
            data.push(w); data.push(h); data.push(0); data.push(0);
            wr16(&mut data, planes); wr16(&mut data, bpp);
            wr(&mut data, size);
            wr16(&mut data, (2 + e) as u16);
        }
        while (data.len() as u32 & 3) != 0 {
            data.push(0);
        }
        // icon blobs
        for b in &blobs { data.extend_from_slice(b); }
    }
    // manifest blob. The loader's SxS parser is strict: it rejects a leading
    // space before the <assembly> root element ("Invalid Xml syntax" on
    // line 2). We normalize the RC-decoded XML: strip whitespace at both ends
    // and collapse runs of whitespace to single spaces, then emit it after the
    // XML declaration with no leading whitespace.
    if with_manifest {
        if let Ok(path) = std::env::var("WINDRES_MANIFEST_FILE") {
            let raw = std::fs::read(&path).unwrap_or_else(|e| die(&format!("cannot read manifest file {path}: {e}")));
            manifest_off = data.len() as u32;
            data.extend_from_slice(&raw);
        } else {
            let normalized: String = manifest_xml
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
            data.extend_from_slice(XML_DECL.as_bytes());
            data.extend_from_slice(normalized.as_bytes());
        }
    }

    // ---- wrap as COFF object (symbol table + ADDR32NB relocations) ----
    // Real windres objects carry ONE ADDR32NB relocation per resource DATA
    // ENTRY offset field: PE resource data-entry offsets are image-base-
    // relative RVAs, while directory offsets are section-relative. The addend
    // lives in the field (the baked section-relative offset); lld adds the
    // .rsrc section RVA, producing the correct absolute RVA in the image.
    // Without these relocations the loader reads manifest/icon data from
    // (image_base + small_offset) -> garbage -> SxS activation fails ->
    // STATUS_INVALID_IMAGE_FORMAT (0xC000007B) at process start.
    let data_size = data.len() as u32;
    let n_relocs = data_entry_off_positions.len() as u16;
    let relocs_size = (n_relocs as u32) * 10;
    let obj_header_size = 20u32 + 40u32;
    let relocs_start = (obj_header_size + data_size + 3) & !3;
    let symtab_start = (relocs_start + relocs_size + 3) & !3;
    let mut obj: Vec<u8> = Vec::new();
    obj.extend_from_slice(&0x8664u16.to_le_bytes());       // machine x64
    obj.extend_from_slice(&1u16.to_le_bytes());            // one section
    obj.extend_from_slice(&0u32.to_le_bytes());            // timestamp
    obj.extend_from_slice(&symtab_start.to_le_bytes());    // ptr to symbols
    obj.extend_from_slice(&1u32.to_le_bytes());            // one symbol (.rsrc section)
    obj.extend_from_slice(&0u16.to_le_bytes());            // no optional header
    obj.extend_from_slice(&0u16.to_le_bytes());            // characteristics
    // section header ".rsrc"
    obj.extend_from_slice(b".rsrc\0\0\0");
    obj.extend_from_slice(&0u32.to_le_bytes());            // virtual size
    obj.extend_from_slice(&0u32.to_le_bytes());            // virtual address
    obj.extend_from_slice(&data_size.to_le_bytes());       // size of raw data
    obj.extend_from_slice(&obj_header_size.to_le_bytes()); // ptr to raw data
    obj.extend_from_slice(&relocs_start.to_le_bytes());    // ptr to relocations
    obj.extend_from_slice(&0u32.to_le_bytes());            // ptr to line numbers
    obj.extend_from_slice(&n_relocs.to_le_bytes());        // num relocations
    obj.extend_from_slice(&0u16.to_le_bytes());            // num line numbers
    obj.extend_from_slice(&0x42000040u32.to_le_bytes());   // CNT_INITIALIZED_DATA|MEM_READ|MEM_DISCARDABLE
    obj.extend_from_slice(&data);
    while (obj.len() as u32) < relocs_start { obj.push(0); }
    // relocations: IMAGE_REL_AMD64_ADDR32NB (0x0003) at each data-entry
    // offset field; symbol index 0 = the .rsrc section symbol below.
    for pos in &data_entry_off_positions {
        obj.extend_from_slice(&pos.to_le_bytes());         // virtual address (section-relative)
        obj.extend_from_slice(&0u32.to_le_bytes());        // symbol table index 0
        obj.extend_from_slice(&0x0003u16.to_le_bytes());   // ADDR32NB
    }
    while (obj.len() as u32) < symtab_start { obj.push(0); }
    // symbol table: one section symbol for .rsrc (value 0 -> its address is
    // the section's RVA in the linked image; the ADDR32NB addend picks the
    // offset inside the section)
    obj.extend_from_slice(b".rsrc\0\0\0");                 // name (8 bytes)
    obj.extend_from_slice(&0u32.to_le_bytes());            // value = 0
    obj.extend_from_slice(&1u16.to_le_bytes());            // section number 1
    obj.extend_from_slice(&0u16.to_le_bytes());            // type
    obj.push(3u8);                                         // storage class STATIC
    obj.push(0u8);                                         // no aux symbols
    // COFF object files must end with the 4-byte string-table size field
    // (0 when there is no string table); without it tools misparse the tail.
    obj.extend_from_slice(&0u32.to_le_bytes());

    fs::write(&out_path, &obj).unwrap_or_else(|e| die(&format!("cannot write {out_path}: {e}")));
    std::process::exit(0);
}

fn die(msg: &str) -> ! {
    eprintln!("windres-shim: {msg}");
    std::process::exit(1);
}
