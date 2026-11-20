//! 门铃往坏里测（LM-4）：`$XDG_RUNTIME_DIR` 合用的先用；不在、太宽、太长的退到自己在 tmp 下建的目录；那个目录被人抢先建了
//! （权限宽、属主不对、是个链接）的不用、不绑；经链接、`..` 的路径敲得到同一个门铃；崩溃留下的套接字删了重绑，有人在听的不拆；
//! 队列满了敲的一方不卡；没人听、不是套接字的悄悄过去；放的时候只删自己绑的那一个；删会话时删没人听的。

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::*;

/// 测试自己的临时目录：`run`（当 `$XDG_RUNTIME_DIR`，0700）、`tmp`（当 `/tmp`）、`sessions/s`（当会话目录）。丢掉时删掉。
struct Root(PathBuf);

impl Root {
    fn new() -> Root {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        // 系统的临时目录太长的（macOS 是 `/var/folders/…/T/`，五十来个字节）改放 `/tmp`：这里的测试自己要在下面绑
        // 套接字，地址有 103 字节的上限；放不下不是程序的事（真用时退路就是 `/tmp`）。
        let base = std::env::temp_dir();
        let base = if base.as_os_str().len() > 24 {
            PathBuf::from("/tmp")
        } else {
            base
        };
        let dir = base.join(format!("lm-bell-{}-{n}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("tmp")).expect("建得了");
        fs::create_dir_all(dir.join("sessions/s")).expect("建得了");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir.join("run"))
            .expect("建得了");
        Root(dir)
    }

    fn bell(&self) -> Bell {
        Bell::new(Some(self.0.join("run")), self.0.join("tmp"))
    }

    fn session(&self) -> PathBuf {
        self.0.join("sessions/s")
    }

    /// 自己在 tmp 下的目录。
    fn own(&self) -> PathBuf {
        self.0.join("tmp").join(format!("litemiyu-{}", euid()))
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

/// 套接字里有没有一个字节（不等）。
fn rung(socket: &UnixDatagram) -> bool {
    let mut buffer = [0u8; 8];
    socket.recv(&mut buffer).is_ok()
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).expect("在").permissions().mode() & 0o777
}

#[test]
fn a_private_runtime_dir_comes_first_and_rings() {
    let root = Root::new();
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    assert_eq!(
        bound.placed.path.parent(),
        Some(root.0.join("run").as_path())
    );
    let file = bound
        .placed
        .path
        .file_name()
        .expect("有名字")
        .to_string_lossy();
    assert!(
        file.starts_with("litemiyu-") && file.ends_with(".sock") && file.len() == 9 + 16 + 5,
        "{file}"
    );
    assert_eq!(mode(&bound.placed.path), 0o600, "别人不可写");
    assert!(!root.own().exists(), "用不着退路就不建");
    assert!(!rung(&bound.socket));
    ring(&root.bell(), &root.session());
    assert!(rung(&bound.socket), "敲得响");
}

#[test]
fn an_unfit_runtime_dir_falls_back_to_an_own_private_folder() {
    // 没设的、不在的、权限宽的、是个文件的：都退到 tmp 下自己的目录。
    let root = Root::new();
    let missing = Bell::new(None, root.0.join("tmp"));
    let gone = Bell::new(Some(root.0.join("nowhere")), root.0.join("tmp"));
    fs::create_dir(root.0.join("wide")).expect("建得了");
    fs::set_permissions(root.0.join("wide"), fs::Permissions::from_mode(0o755)).expect("改得了");
    let wide = Bell::new(Some(root.0.join("wide")), root.0.join("tmp"));
    fs::write(root.0.join("file"), "").expect("写得进");
    let file = Bell::new(Some(root.0.join("file")), root.0.join("tmp"));
    for bell in [missing, gone, wide, file] {
        let bound = bind(&bell, &root.session()).expect("退得到");
        assert_eq!(
            bound.placed.path.parent(),
            Some(root.own().as_path()),
            "{bell:?}"
        );
        assert_eq!(mode(&root.own()), 0o700);
        ring(&bell, &root.session());
        assert!(rung(&bound.socket), "{bell:?}");
        bound.placed.release();
        assert!(!bound.placed.path.exists());
    }
    assert_eq!(
        fs::read_dir(root.0.join("wide")).expect("在").count(),
        0,
        "不合用的那一处什么都没放"
    );
}

#[test]
fn a_runtime_dir_owned_by_someone_else_is_not_used() {
    let root = Root::new();
    let dir = fs::canonicalize(root.session()).expect("在");
    let mine = spots(&root.bell(), &dir, euid());
    assert_eq!(
        mine.len(),
        1,
        "自己的：只有 run 那一处（tmp 下的目录还没建）"
    );
    assert!(
        spots(&root.bell(), &dir, euid() + 1).is_empty(),
        "属主不是自己的 run 不用"
    );
}

#[test]
fn a_folder_someone_else_put_in_tmp_first_is_not_used() {
    let root = Root::new();
    let bell = Bell::new(None, root.0.join("tmp"));
    // 权限宽：别人能进。
    fs::create_dir(root.own()).expect("建得了");
    fs::set_permissions(root.own(), fs::Permissions::from_mode(0o777)).expect("改得了");
    assert!(bind(&bell, &root.session()).is_none(), "权限宽的不用");
    assert_eq!(mode(&root.own()), 0o777, "也不去改它");
    assert_eq!(fs::read_dir(root.own()).expect("在").count(), 0);
    fs::remove_dir(root.own()).expect("删得了");
    // 链接：指向一个合格的目录也不用。
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.0.join("elsewhere"))
        .expect("建得了");
    std::os::unix::fs::symlink(root.0.join("elsewhere"), root.own()).expect("建得了链接");
    assert!(bind(&bell, &root.session()).is_none(), "链接不用");
    assert_eq!(
        fs::read_dir(root.0.join("elsewhere")).expect("在").count(),
        0
    );
    // 属主不是自己：照别的用户的名字建的目录，属主是自己，对那个用户来说就是别人抢先建的。
    let other = euid() + 1;
    assert!(own_folder(&root.0.join("tmp"), other, true).is_none());
    // 发的一方也不往那里敲，敲了也不出事。
    ring(&bell, &root.session());
}

#[test]
fn too_long_addresses_fall_back_and_then_go_without() {
    let root = Root::new();
    let deep = |base: &Path| {
        let dir = base.join("x".repeat(60)).join("y".repeat(60));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .expect("建得了");
        dir
    };
    let long_run = deep(&root.0.join("run"));
    let bell = Bell::new(Some(long_run.clone()), root.0.join("tmp"));
    let bound = bind(&bell, &root.session()).expect("退到 tmp");
    assert_eq!(bound.placed.path.parent(), Some(root.own().as_path()));
    assert!(bound.placed.path.as_os_str().len() <= MAX_PATH);
    drop(bound);
    let long_tmp = deep(&root.0.join("tmp"));
    let bell = Bell::new(Some(long_run), long_tmp);
    assert!(bind(&bell, &root.session()).is_none(), "两处都太长：不绑");
    ring(&bell, &root.session());
}

#[test]
fn a_link_or_dots_on_the_way_ring_the_same_bell() {
    let root = Root::new();
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    std::os::unix::fs::symlink(root.0.join("sessions"), root.0.join("linked")).expect("建得了链接");
    for way in [
        root.0.join("linked/s"),
        root.0.join("sessions/../sessions/s/"),
        root.0.join("tmp/../linked/./s"),
    ] {
        assert!(!rung(&bound.socket));
        ring(&root.bell(), &way);
        assert!(rung(&bound.socket), "{}", way.display());
    }
}

#[test]
fn a_stale_socket_is_rebound_and_a_live_one_is_left_alone() {
    let root = Root::new();
    let first = bind(&root.bell(), &root.session()).expect("绑得上");
    let path = first.placed.path.clone();
    // 崩溃：套接字关了，文件留着。
    drop(first.socket);
    assert!(path.exists());
    let second = bind(&root.bell(), &root.session()).expect("删了重绑");
    assert_eq!(second.placed.path, path);
    // 有人在听的不拆：这一处不用，退到下一处。
    let third = bind(&root.bell(), &root.session()).expect("退到 tmp");
    assert_eq!(third.placed.path.parent(), Some(root.own().as_path()));
    ring(&root.bell(), &root.session());
    assert!(rung(&second.socket), "原来那个照样敲得响");
    assert!(rung(&third.socket), "两处都敲");
    // 放的时候只删自己绑的那一个：被换掉了的不动。
    first.placed.release();
    assert!(path.exists(), "崩溃那一份的记录删不了后来绑的");
    second.placed.release();
    assert!(!path.exists());
}

#[test]
fn ringing_a_full_bell_does_not_block() {
    let root = Root::new();
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    let (bell, dir) = (root.bell(), root.session());
    let (done, finished) = std::sync::mpsc::channel();
    // 从不读：队列早满了。阻塞的敲法会卡在第十几下，这里等不到。
    std::thread::spawn(move || {
        for _ in 0..5000 {
            ring(&bell, &dir);
        }
        done.send(()).expect("等的一头在");
    });
    finished
        .recv_timeout(Duration::from_secs(60))
        .expect("敲满了也不卡");
    assert!(rung(&bound.socket));
}

#[test]
fn ringing_nobody_is_silent() {
    let root = Root::new();
    // 没绑、会话目录不在、地址上是个普通文件、套接字没人听：都悄悄过去。
    ring(&root.bell(), &root.session());
    ring(&root.bell(), &root.0.join("sessions/gone"));
    let dir = fs::canonicalize(root.session()).expect("在");
    let spot = spots(&root.bell(), &dir, euid())
        .into_iter()
        .next()
        .expect("有一处");
    fs::write(&spot, "").expect("写得进");
    ring(&root.bell(), &root.session());
    fs::remove_file(&spot).expect("删得了");
    drop(UnixDatagram::bind(&spot).expect("绑得上"));
    ring(&root.bell(), &root.session());
}

#[test]
fn forgetting_removes_only_sockets_nobody_listens_on() {
    let root = Root::new();
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    forget(&root.bell(), &root.session());
    assert!(bound.placed.path.exists(), "有人在听的不删");
    let path = bound.placed.path.clone();
    drop(bound);
    forget(&root.bell(), &root.session());
    assert!(!path.exists(), "崩溃留下的删掉");
}

#[test]
fn names_differ_by_dir_and_stay_short() {
    let a = name(Path::new("/a/sessions/1"));
    assert_eq!(a, name(Path::new("/a/sessions/1")));
    assert_ne!(a, name(Path::new("/a/sessions/2")));
    assert_ne!(a, name(Path::new("/b/sessions/1")));
    assert_eq!(a.len(), 16);
    assert!(
        a.bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
}

/// 在 `dir` 里 `name` 上绑一个套接字再关掉：崩溃留下的那种，没人在听。
fn stale_at(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    drop(UnixDatagram::bind(&path).expect("绑得上"));
    path
}

/// 门铃的名字（16 位十六进制），第 `n` 个。
fn hex(n: u64) -> String {
    format!("{n:016x}")
}

#[test]
fn binding_clears_bells_nobody_listens_on_in_both_places() {
    // 建会话、打开会话绑门铃时（LM-14）：运行时目录、tmp 下自己的目录里别的会话崩溃留下的套接字删掉。
    let root = Root::new();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.own())
        .expect("建得了");
    let run = root.0.join("run");
    let in_run = stale_at(&run, &format!("litemiyu-{}.sock", hex(1)));
    let in_own = stale_at(&root.own(), &format!("{}.sock", hex(2)));
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    assert!(!in_run.exists(), "运行时目录里没人听的删掉");
    assert!(!in_own.exists(), "tmp 下自己的目录里没人听的删掉");
    assert!(bound.placed.path.exists(), "自己刚绑的在");
    ring(&root.bell(), &root.session());
    assert!(rung(&bound.socket));
}

#[test]
fn binding_leaves_live_bells_and_other_files_alone() {
    let root = Root::new();
    let run = root.0.join("run");
    // 有人在听的。
    let live = run.join(format!("litemiyu-{}.sock", hex(1)));
    let listening = UnixDatagram::bind(&live).expect("绑得上");
    listening.set_nonblocking(true).expect("设得了");
    // 名字对、不是套接字的：普通文件、目录、指向一个没人听的套接字的链接。
    let file = run.join(format!("litemiyu-{}.sock", hex(2)));
    fs::write(&file, "").expect("写得进");
    let folder = run.join(format!("litemiyu-{}.sock", hex(3)));
    fs::create_dir(&folder).expect("建得了");
    let target = stale_at(&root.0, "elsewhere.sock");
    let link = run.join(format!("litemiyu-{}.sock", hex(4)));
    std::os::unix::fs::symlink(&target, &link).expect("建得了链接");
    // 名字不对的没人听的套接字：别的进程换门铃时旁边那个临时名字（`<地址>.<进程号>`）、大写、长短不对、别的程序的。
    let names = [
        format!("litemiyu-{}.sock.4242", hex(5)),
        format!("litemiyu-{}.sock", hex(6).to_uppercase().replace('0', "A")),
        format!("litemiyu-{}.sock", &hex(7)[1..]),
        "other-0000000000000008.sock".to_owned(),
    ];
    let odd: Vec<PathBuf> = names.iter().map(|name| stale_at(&run, name)).collect();
    // 连都连不上的（没有写权限）：判不了有没有人在听，不动。属主不是自己的做不出来（要 root），拿它代替「别人的」。
    let shut = stale_at(&run, &format!("litemiyu-{}.sock", hex(9)));
    fs::set_permissions(&shut, fs::Permissions::from_mode(0o000)).expect("改得了");
    let bound = bind(&root.bell(), &root.session()).expect("绑得上");
    for path in [&live, &file, &folder, &link, &target]
        .into_iter()
        .chain(&odd)
    {
        assert!(
            fs::symlink_metadata(path).is_ok(),
            "不动：{}",
            path.display()
        );
    }
    if euid() != 0 {
        assert!(fs::symlink_metadata(&shut).is_ok(), "连不上的不动");
    }
    let knock = UnixDatagram::unbound().expect("开得了");
    knock.send_to(b"!", &live).expect("还敲得响");
    assert!(rung(&listening), "有人在听的照样收得到");
    drop(bound);
}

#[test]
fn a_crowded_folder_is_cleared_a_part_at_a_time() {
    // 攒了很多的：一次只看 SWEEP 个，不报错；剩下的留给下一次绑门铃。
    let root = Root::new();
    let run = root.0.join("run");
    let total = SWEEP as u64 + 36;
    for n in 0..total {
        stale_at(&run, &format!("litemiyu-{}.sock", hex(0x100 + n)));
    }
    let left = || {
        fs::read_dir(&run)
            .expect("在")
            .filter(|entry| {
                let name = entry.as_ref().expect("读得了").file_name();
                name.to_string_lossy()
                    .starts_with("litemiyu-00000000000001")
            })
            .count()
    };
    let first = bind(&root.bell(), &root.session()).expect("绑得上");
    assert_eq!(left(), 36, "这一次只清了 {SWEEP} 个");
    fs::create_dir(root.0.join("sessions/t")).expect("建得了");
    let second = bind(&root.bell(), &root.0.join("sessions/t")).expect("绑得上");
    assert_eq!(left(), 0, "下一次接着清");
    drop((first, second));
}

#[test]
fn only_sockets_of_the_same_owner_are_cleared() {
    // 属主不是自己的做不出来（要 root）：照别的用户的编号去清，自己的文件对那个用户来说就是别人的。
    let root = Root::new();
    let run = root.0.join("run");
    let path = stale_at(&run, &format!("litemiyu-{}.sock", hex(1)));
    let mut budget = SWEEP;
    clear(&run, "litemiyu-", euid() + 1, None, &mut budget);
    assert!(path.exists(), "属主不对的不动");
    assert_eq!(budget, SWEEP - 1, "看过了，算一个");
    clear(&run, "litemiyu-", euid(), None, &mut budget);
    assert!(!path.exists(), "自己的删掉");
}
