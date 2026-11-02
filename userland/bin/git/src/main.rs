#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use portix_rt::{
    println, print, File, O_RDONLY, O_WRONLY, O_CREAT, O_TRUNC,
    mkdir, chdir, fs::Stat
};

fn parse_args(argc: i32, argv: *const *const u8) -> Vec<String> {
    let mut args = Vec::new();
    for i in 0..argc {
        unsafe {
            let ptr = *argv.add(i as usize);
            if ptr.is_null() { break; }
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            let bytes = core::slice::from_raw_parts(ptr, len);
            if let Ok(s) = core::str::from_utf8(bytes) {
                args.push(s.to_string());
            }
        }
    }
    args
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xefcdab89;
    let mut h2: u32 = 0x98badcfe;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xc3d2e1f0;

    let ml = (data.len() as u64) * 8;
    let mut msg = Vec::with_capacity(data.len() + 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]);
        }
        for i in 16..80 {
            w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19   => ((b & c) | ((!b) & d), 0x5a827999),
                20..=39  => (b ^ c ^ d, 0x6ed9eba1),
                40..=59  => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _        => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn read_file_string(path: &str) -> Option<String> {
    let mut f = File::open(path, O_RDONLY).ok()?;
    let mut buf = [0u8; 4096];
    let n = f.read(&mut buf).unwrap_or(0);
    core::str::from_utf8(&buf[..n]).ok().map(|s| s.to_string())
}

fn write_file_string(path: &str, content: &str) -> bool {
    if let Ok(mut f) = File::open(path, O_WRONLY | O_CREAT | O_TRUNC) {
        let _ = f.write(content.as_bytes());
        true
    } else {
        false
    }
}

fn file_exists(path: &str) -> bool {
    portix_rt::fs::stat(path).is_ok()
}

fn cmd_init(dir: Option<&str>) -> i32 {
    let base = dir.unwrap_or(".");
    if base != "." {
        let _ = mkdir(base, 0o755);
        let _ = chdir(base);
    }
    let _ = mkdir(".git", 0o755);
    let _ = mkdir(".git/objects", 0o755);
    let _ = mkdir(".git/refs", 0o755);
    let _ = mkdir(".git/refs/heads", 0o755);
    write_file_string(".git/HEAD", "ref: refs/heads/main\n");
    write_file_string(".git/config", "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n");
    println!("Initialized empty Git repository in .git/");
    0
}

fn cmd_status() -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository (or any of the parent directories): .git");
        return 128;
    }
    let head = read_file_string(".git/HEAD").unwrap_or_else(|| "ref: refs/heads/main\n".to_string());
    let branch = head.trim().strip_prefix("ref: refs/heads/").unwrap_or("main");
    println!("On branch {}", branch);

    let branch_ref = format!(".git/refs/heads/{}", branch);
    if !file_exists(&branch_ref) {
        println!("\nNo commits yet\n");
    }

    if let Some(index) = read_file_string(".git/index") {
        println!("Changes to be committed:");
        for line in index.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                println!("\tnew file:   {}", parts[1]);
            }
        }
    } else {
        println!("nothing to commit (create/copy files and use \"git add\" to track)");
    }
    0
}

fn cmd_add(files: &[String]) -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    if files.is_empty() {
        println!("Nothing specified, nothing added.");
        return 0;
    }

    let mut current_index = read_file_string(".git/index").unwrap_or_default();
    for path in files {
        if let Some(content) = read_file_string(path) {
            let hash_bytes = sha1(content.as_bytes());
            let hash_str = to_hex(&hash_bytes);
            let dir_prefix = format!(".git/objects/{}", &hash_str[0..2]);
            let _ = mkdir(&dir_prefix, 0o755);
            let obj_path = format!("{}/{}", dir_prefix, &hash_str[2..]);
            write_file_string(&obj_path, &content);

            let mut new_lines = Vec::new();
            for line in current_index.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == path {
                    continue;
                }
                new_lines.push(line.to_string());
            }
            new_lines.push(format!("{} {}", hash_str, path));
            current_index = new_lines.join("\n") + "\n";
        } else {
            println!("fatal: pathspec '{}' did not match any files", path);
            return 1;
        }
    }
    write_file_string(".git/index", &current_index);
    0
}

fn cmd_commit(msg: &str) -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    let index = match read_file_string(".git/index") {
        Some(idx) if !idx.trim().is_empty() => idx,
        _ => {
            println!("no changes added to commit (use \"git add\")");
            return 1;
        }
    };

    let tree_hash = to_hex(&sha1(index.as_bytes()));
    let head = read_file_string(".git/HEAD").unwrap_or_else(|| "ref: refs/heads/main\n".to_string());
    let branch = head.trim().strip_prefix("ref: refs/heads/").unwrap_or("main");
    let branch_ref = format!(".git/refs/heads/{}", branch);
    let parent = read_file_string(&branch_ref);

    let mut commit_content = format!("tree {}\n", tree_hash);
    if let Some(p) = parent {
        let p_clean = p.trim();
        if !p_clean.is_empty() {
            commit_content.push_str(&format!("parent {}\n", p_clean));
        }
    }
    commit_content.push_str("author portix <user@portix> 1774300800 +0000\n");
    commit_content.push_str("committer portix <user@portix> 1774300800 +0000\n\n");
    commit_content.push_str(msg);
    commit_content.push('\n');

    let commit_hash = to_hex(&sha1(commit_content.as_bytes()));
    let dir_prefix = format!(".git/objects/{}", &commit_hash[0..2]);
    let _ = mkdir(&dir_prefix, 0o755);
    write_file_string(&format!("{}/{}", dir_prefix, &commit_hash[2..]), &commit_content);
    write_file_string(&branch_ref, &format!("{}\n", commit_hash));

    // Append to commit log history
    let mut log = read_file_string(".git/LOG").unwrap_or_default();
    let entry = format!("commit {}\nAuthor: portix <user@portix>\nDate:   2026-09-21\n\n    {}\n\n", commit_hash, msg);
    log = entry + &log;
    write_file_string(".git/LOG", &log);

    let short_hash = &commit_hash[0..7];
    println!("[{} {}] {}", branch, short_hash, msg);
    0
}

fn cmd_log() -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    if let Some(log) = read_file_string(".git/LOG") {
        print!("{}", log);
    } else {
        println!("fatal: your current branch does not have any commits yet");
        return 1;
    }
    0
}

fn cmd_branch(name: Option<&str>) -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    let head = read_file_string(".git/HEAD").unwrap_or_else(|| "ref: refs/heads/main\n".to_string());
    let current_branch = head.trim().strip_prefix("ref: refs/heads/").unwrap_or("main");

    if let Some(new_branch) = name {
        let current_ref = format!(".git/refs/heads/{}", current_branch);
        let commit = read_file_string(&current_ref).unwrap_or_default();
        write_file_string(&format!(".git/refs/heads/{}", new_branch), &commit);
        println!("Created branch '{}'", new_branch);
    } else {
        println!("* {}", current_branch);
    }
    0
}

fn cmd_checkout(branch: &str) -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    let ref_path = format!(".git/refs/heads/{}", branch);
    if !file_exists(&ref_path) {
        println!("error: pathspec '{}' did not match any file(s) known to git", branch);
        return 1;
    }
    write_file_string(".git/HEAD", &format!("ref: refs/heads/{}\n", branch));
    println!("Switched to branch '{}'", branch);
    0
}

fn cmd_diff() -> i32 {
    if !file_exists(".git") {
        println!("fatal: not a git repository");
        return 128;
    }
    if let Some(index) = read_file_string(".git/index") {
        for line in index.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let staged_hash = parts[0];
                let path = parts[1];
                if let Some(cur_content) = read_file_string(path) {
                    let cur_hash = to_hex(&sha1(cur_content.as_bytes()));
                    if cur_hash != staged_hash {
                        println!("diff --git a/{} b/{}", path, path);
                        println!("--- a/{}", path);
                        println!("+++ b/{}", path);
                        println!("@@ -1 +1 @@");
                        println!("+{}", cur_content.trim());
                    }
                }
            }
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn main(argc: i32, argv: *const *const u8, _envp: *const *const u8) -> i32 {
    let args = parse_args(argc, argv);
    if args.len() < 2 {
        println!("usage: git <command> [<args>]");
        println!("\nSupported commands:");
        println!("   init      Create an empty Git repository or reinitialize an existing one");
        println!("   status    Show the working tree status");
        println!("   add       Add file contents to the index");
        println!("   commit    Record changes to the repository");
        println!("   log       Show commit logs");
        println!("   branch    List, create, or delete branches");
        println!("   checkout  Switch branches");
        println!("   diff      Show changes between commits, commit and working tree");
        println!("   version   Show git version");
        return 0;
    }

    match args[1].as_str() {
        "--version" | "version" => {
            println!("git version 2.44.0 (Portix OS native git)");
            0
        }
        "init" => {
            let dir = if args.len() > 2 { Some(args[2].as_str()) } else { None };
            cmd_init(dir)
        }
        "status" => cmd_status(),
        "add" => {
            if args.len() < 3 {
                println!("Nothing specified, nothing added.");
                0
            } else {
                cmd_add(&args[2..])
            }
        }
        "commit" => {
            let mut msg = "update";
            for i in 2..args.len() {
                if args[i] == "-m" && i + 1 < args.len() {
                    msg = &args[i + 1];
                    break;
                }
            }
            cmd_commit(msg)
        }
        "log" => cmd_log(),
        "branch" => {
            let name = if args.len() > 2 { Some(args[2].as_str()) } else { None };
            cmd_branch(name)
        }
        "checkout" => {
            if args.len() > 2 {
                cmd_checkout(&args[2])
            } else {
                println!("fatal: missing branch name");
                1
            }
        }
        "diff" => cmd_diff(),
        cmd => {
            println!("git: '{}' is not a git command. See 'git --help'.", cmd);
            1
        }
    }
}
