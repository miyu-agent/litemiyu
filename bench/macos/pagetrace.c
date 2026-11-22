// 探针（只在量的时候用 DYLD_INSERT_LIBRARIES 插进被测程序）：进 main 以前把主程序的 __TEXT 段整个设成不可访问，之后每
// 碰到一页就记下「第几页、是谁碰的（pc）、碰的地址」，放开这一页接着跑。收到 SIGUSR1 时把记下的写进 LM_PAGES_OUT。
// 被测程序自己装 SIGBUS/SIGSEGV 处理函数的那一下被拦掉（不然 Rust 的运行时会把这里的顶掉）。
#include <crt_externs.h>
#include <fcntl.h>
#include <mach-o/loader.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/ucontext.h>
#include <unistd.h>
static char out_path[1024];
static uintptr_t text_at; static uint64_t text_vm, text_len; static size_t page;
struct hit { uint64_t page, pc, addr; };
static struct hit hits[2048]; static volatile int count;
static void fault(int sig, siginfo_t *info, void *raw) {
    uintptr_t addr = (uintptr_t)info->si_addr;
    if (addr < text_at || addr >= text_at + text_len) { signal(sig, SIG_DFL); return; }
    ucontext_t *uc = (ucontext_t *)raw;
    uintptr_t pc = (uintptr_t)uc->uc_mcontext->__ss.__pc;
    uintptr_t base = addr & ~(page - 1);
    mprotect((void *)base, page, PROT_READ | PROT_EXEC);
    int i = __sync_fetch_and_add(&count, 1);
    if (i < 2048) { hits[i].page = (base - text_at) / page; hits[i].pc = pc - text_at + text_vm; hits[i].addr = addr - text_at + text_vm; }
}
static void dump(int sig) {
    (void)sig;
    int fd = open(out_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) return;
    int n = count < 2048 ? count : 2048;
    uint64_t head[3] = {text_vm, page, (uint64_t)n};
    write(fd, head, sizeof head); write(fd, hits, sizeof(struct hit) * (size_t)n); close(fd);
}
static int my_sigaction(int sig, const struct sigaction *act, struct sigaction *old) {
    if ((sig == SIGBUS || sig == SIGSEGV) && act && text_at) { if (old) memset(old, 0, sizeof *old); return 0; }
    return sigaction(sig, act, old);
}
__attribute__((used, section("__DATA,__interpose"))) static struct { void *new_fn, *old_fn; } interpose = {(void *)my_sigaction, (void *)sigaction};
__attribute__((constructor)) static void setup(void) {
    const char *path = getenv("LM_PAGES_OUT");
    if (!path) return;
    strlcpy(out_path, path, sizeof out_path);
    page = (size_t)getpagesize();
    const struct mach_header_64 *mh = (const struct mach_header_64 *)_NSGetMachExecuteHeader();
    const struct load_command *lc = (const struct load_command *)(mh + 1);
    for (uint32_t i = 0; i < mh->ncmds; i++, lc = (const struct load_command *)((const char *)lc + lc->cmdsize)) {
        if (lc->cmd != LC_SEGMENT_64) continue;
        const struct segment_command_64 *seg = (const struct segment_command_64 *)lc;
        if (strcmp(seg->segname, "__TEXT") == 0) { text_vm = seg->vmaddr; text_len = seg->vmsize; }
    }
    struct sigaction sa; memset(&sa, 0, sizeof sa); sa.sa_sigaction = fault; sa.sa_flags = SA_SIGINFO | SA_NODEFER; sigfillset(&sa.sa_mask);
    sigaction(SIGBUS, &sa, 0); sigaction(SIGSEGV, &sa, 0);
    signal(SIGUSR1, dump);
    text_at = (uintptr_t)mh;
    mprotect((void *)text_at, text_len, PROT_NONE);
}
