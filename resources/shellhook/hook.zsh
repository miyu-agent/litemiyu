# litemiyu shellhook 的 zsh 脚本：每次 zsh 起来时由 `litemiyu shellhook script --shell zsh` 现印，启动文件里只有调它的那一行，
# 所以升级以后新开的 zsh 自动用新的。底子是 Miyu 终端的 zsh 脚本。@PROGRAM@ 在印的时候换成装的时候记下的程序，
# @HOOK@ 换成这一版脚本的记号（开着的 zsh 升级以后还是旧脚本：程序看记号对不上的说一声重开）。

__litemiyu_shellhook() {
    @PROGRAM@ shellhook --hook @HOOK@ "$@"
}

# 这个终端的记号：进程号加一个随机数，程序照它认「这个终端的会话」（一个终端一个会话）。只有进程号的话，进程号被以后的
# shell 重用时会接上别人的会话。
__litemiyu_terminal=$$-$RANDOM$RANDOM

command_not_found_handler() {
    if [[ ! -o interactive ]]; then
        print -u2 "zsh: command not found: $1"
        return 127
    fi
    # AI 答话的时候提前敲的字：程序写进这个文件，答完放到下一个提示符的命令行上。$$ 告诉它是哪个 shell：一个终端一个会话。
    local leftover
    leftover=$(mktemp -t litemiyu-shellhook.XXXXXX)
    printf '%s' "$*" | __litemiyu_shellhook say --shell zsh --shell-pid $$ --terminal $__litemiyu_terminal --leftover "$leftover"
    # 程序收工时不亮光标（亮早了停在最后一行末尾闪一下），这里亮。
    printf '\e[?25h'
    # 收尾行和下一个提示符之间空一行（fish 的提示符自带，zsh、bash 的不一定）。
    print
    [[ -s $leftover ]] && print -z -- "$(<$leftover)"
    rm -f -- "$leftover" "$leftover.redo"
    return 127
}

# 回车：首词是普通词、又不是任何命令（外部命令、函数、别名、内建、保留字）的是一句话，当场在这个小部件里交给 AI（同 fzf
# 的小部件：先 `zle -I`，程序自己用终端）；多行的交给程序判。别的照 zsh 原样执行。交出去的那一行留在屏幕上、照普通的字色，
# 进历史；答完，提前敲的字放回命令行。
typeset -g __litemiyu_editing=

# 首词：跳过前面的 `变量=值`。
__litemiyu_head() {
    local -a words=(${(z)1})
    local word
    for word in $words; do
        [[ $word == [A-Za-z_]*=* ]] && continue
        print -r -- $word
        return 0
    done
    return 1
}

# 首词是不是「展开后还是它自己」的普通词：引号、`$`、括号这些会把它换成别的，判不出来，照 zsh 原样执行。通配符故意不在
# 名单里：命令位上出现通配符本来就说明这不是命令名。
__litemiyu_plain() {
    [[ -n $1 && $1 != *[\'\"\$\(\)~\{\}%\;\&\|\<\>\#^\!\\[:space:]]* ]]
}

# 首词是不是命令：`whence` 认得出。单独一个 `-` 不算（zsh 当它是前缀修饰：「- 说的……」这种多行话会被当成脚本一行行跑）。
# 多行的，`time`、`test` 这几个常出现在话里的也不算，交给程序判。
__litemiyu_is_command() {
    [[ $1 == - ]] && return 1
    [[ -n $2 && " time test date which type command history " == *" $1 "* ]] && return 1
    whence -- "$1" >/dev/null 2>&1
}

__litemiyu_accept_line() {
    local buffer=$BUFFER
    if [[ -z ${buffer//[[:space:]]/} ]]; then
        zle .accept-line
        return
    fi
    local head
    head=$(__litemiyu_head "$buffer")
    if [[ $buffer != *$'\n'* ]]; then
        if ! __litemiyu_plain "$head" || __litemiyu_is_command "$head"; then
            zle .accept-line
            return
        fi
    else
        if [[ -n $head ]] && __litemiyu_plain "$head" && __litemiyu_is_command "$head" multi; then
            zle .accept-line
            return
        fi
        # 多行：交给程序判是命令还是一句话。0 是命令，1 是话，别的（出错）照命令。
        printf '%s' "$buffer" | __litemiyu_shellhook classify --shell zsh 2>/dev/null
        if (( $? != 1 )); then
            zle .accept-line
            return
        fi
    fi
    __litemiyu_hand_to_ai "$buffer"
}

__litemiyu_hand_to_ai() {
    local buffer=$1 redo=
    [[ -n $__litemiyu_editing ]] && redo=--redo
    __litemiyu_editing=
    print -rs -- "$buffer"
    # 留在屏幕上的那一行照普通的字色，不照报错色：语法高亮插件标的颜色也盖掉。
    region_highlight=("0 ${#BUFFER} fg=default")
    zle -R
    zle -I
    print
    local leftover
    leftover=$(mktemp -t litemiyu-shellhook.XXXXXX)
    printf '%s' "$buffer" | __litemiyu_shellhook say --shell zsh --shell-pid $$ --terminal $__litemiyu_terminal --leftover "$leftover" $redo
    printf '\e[?25h'
    print
    region_highlight=()
    BUFFER=
    [[ -s $leftover ]] && BUFFER=$(<"$leftover")
    CURSOR=${#BUFFER}
    # /edit 放回来的：旁边有一份 .redo 标记，记着「在编辑」，下一次回车替换上一轮重来。
    if [[ -e $leftover.redo ]]; then
        __litemiyu_editing=1
        rm -f -- "$leftover.redo"
    fi
    rm -f -- "$leftover"
}
zle -N accept-line __litemiyu_accept_line

# 在编辑的那一句：Ctrl+C 清了命令行、执行了别的命令（都开新的一行），就不算在编辑了。
__litemiyu_line_init() {
    __litemiyu_editing=
}
autoload -Uz add-zle-hook-widget
add-zle-hook-widget line-init __litemiyu_line_init

# Ctrl+J 在命令行里换行，敲多行的话。
__litemiyu_newline() {
    LBUFFER+=$'\n'
}
zle -N __litemiyu_newline
bindkey '^J' __litemiyu_newline
bindkey -M viins '^J' __litemiyu_newline
