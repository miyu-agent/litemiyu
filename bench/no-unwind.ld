/* 发版不要展开表（瘦身 五）：bench/order.py 链接时交给 lld（-T）。
 *
 * 发行版崩了直接退出（panic = "abort"），展开表只剩崩的时候打调用栈一个用处；崩在哪个文件哪一行的那句话是 panic 的消息，
 * 不靠它。稳定版的 rustc 去不掉：标准库是预编译的、带调试信息的位码，整体优化以后整份程序的每个函数都照样写进 .eh_frame。
 * 所以在链接这一步丢掉三段：展开表、它的索引、各函数的异常表（只有展开表指着它）。
 *
 * INSERT 让这几行加在 lld 默认的布局上，别的段照旧；索引段没了，程序头里的 GNU_EH_FRAME 链接器也不再写。
 */
SECTIONS
{
  /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) *(.gcc_except_table .gcc_except_table.*) }
}
INSERT AFTER .text;
