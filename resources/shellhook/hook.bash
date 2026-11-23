# litemiyu shellhook 的 bash 脚本：每次 bash 起来时由 `litemiyu shellhook script --shell bash` 现印，启动文件里只有调它的那一行，
# 所以升级以后新开的 bash 自动用新的。底子是 Miyu 终端的 bash 脚本。@PROGRAM@ 在印的时候换成装的时候记下的程序，
# @HOOK@ 换成这一版脚本的记号（开着的 bash 升级以后还是旧脚本：程序看记号对不上的说一声重开）。

__litemiyu_shellhook() {
    @PROGRAM@ shellhook --hook @HOOK@ "$@"
}

# 这个终端的记号：进程号加一个随机数，程序照它认「这个终端的会话」（一个终端一个会话）。只有进程号的话，进程号被以后的
# shell 重用时会接上别人的会话。
__litemiyu_terminal=$$-$RANDOM$RANDOM

command_not_found_handle() {
    case $- in
        *i*) ;;
        *)
            printf 'bash: %s: command not found\n' "$1" >&2
            return 127
            ;;
    esac
    # $$ 是这个 bash（这里在子 shell 里也不变）：一个终端一个会话。这条路上提前敲的字放不回 readline，不收。
    printf '%s' "$*" | __litemiyu_shellhook say --shell bash --shell-pid $$ --terminal $__litemiyu_terminal --reports
    # 程序收工时不亮光标（亮早了停在最后一行末尾闪一下），这里亮。
    printf '\e[?25h'
    # 收尾行和下一个提示符之间空一行（fish 的提示符自带，bash 的不一定）。
    printf '\n'
    return 127
}

# 回车：readline 一个键里没法先判再决定执不执行，所以回车绑成「先跑 __litemiyu_accept_line，再执行这一行」两个键。首词是
# 普通词、又不是任何命令（`type` 认不出）的是一句话，当场在这里交给 AI，把第二个键换成什么都不做（这一下不执行）；多行的
# 交给程序判。别的照 bash 原样执行。bash 跑之前擦掉了输入的那一行（只擦提示符最后一行）：照普通的字色重画，答完把提示符
# 前面几行补上（bash 只重画最后一行），提前敲的字放回命令行。
__litemiyu_editing=
__litemiyu_busy=
__litemiyu_report_pending=

# 首词：跳过前面的 `变量=值`。
__litemiyu_head() {
    local -a words
    read -r -a words <<<"$1"
    local word
    for word in "${words[@]}"; do
        [[ $word =~ ^[A-Za-z_][A-Za-z0-9_]*= ]] && continue
        printf '%s' "$word"
        return 0
    done
    return 1
}

# 首词是不是「展开后还是它自己」的普通词：引号、`$`、括号这些会把它换成别的，判不出来，照 bash 原样执行。
__litemiyu_plain() {
    [[ -n $1 && $1 != *[\'\"\$\(\)~\{\}%\;\&\|\<\>\#^\!\\[:space:]]* ]]
}

# 首词是不是命令：`type` 认得出。单独一个 `-` 不算。多行的，`time`、`test` 这几个常出现在话里的也不算，交给程序判。
__litemiyu_is_command() {
    [[ $1 == - ]] && return 1
    [[ -n $2 && " time test date which type command history " == *" $1 "* ]] && return 1
    type -t -- "$1" >/dev/null 2>&1
}

__litemiyu_is_talk() {
    local buffer=$1 head
    [[ -z ${buffer//[[:space:]]/} ]] && return 1
    head=$(__litemiyu_head "$buffer")
    if [[ $buffer != *$'\n'* ]]; then
        __litemiyu_plain "$head" && ! __litemiyu_is_command "$head"
        return
    fi
    [[ -n $head ]] && __litemiyu_plain "$head" && __litemiyu_is_command "$head" multi && return 1
    # 多行：交给程序判是命令还是一句话。0 是命令，1 是话，别的（出错）照命令。
    printf '%s' "$buffer" | __litemiyu_shellhook classify --shell bash 2>/dev/null
    (( $? == 1 ))
}

__litemiyu_accept_line() {
    if ! __litemiyu_is_talk "$READLINE_LINE"; then
        __litemiyu_editing=
        bind '"\C-x\C-b": accept-line'
        return
    fi
    bind '"\C-x\C-b": ""'
    local buffer=$READLINE_LINE redo=
    [[ -n $__litemiyu_editing ]] && redo=--redo
    __litemiyu_editing=
    history -s -- "$buffer"
    local prompt=${PS1@P}
    prompt=${prompt//$'\001'/}
    prompt=${prompt//$'\002'/}
    # 交出去的那句话照普通的字色重画在提示符最后一行后面（不照报错色），折下来的几行缩两格。
    local first=${buffer%%$'\n'*} line
    printf '%s\e[0m%s' "${prompt##*$'\n'}" "$first"
    if [[ $buffer == *$'\n'* ]]; then
        while IFS= read -r line; do
            printf '\n  %s' "$line"
        done <<<"${buffer#*$'\n'}"
    fi
    printf '\e[0m\n'
    local leftover
    leftover=$(mktemp -t litemiyu-shellhook.XXXXXX)
    __litemiyu_busy=1
    printf '%s' "$buffer" | __litemiyu_shellhook say --shell bash --shell-pid $$ --terminal $__litemiyu_terminal --reports --leftover "$leftover" $redo
    printf '\e[?25h\n'
    # AI 答话的时候来的后台回报：答完印。
    if [[ -n $__litemiyu_report_pending ]]; then
        __litemiyu_report_pending=
        __litemiyu_shellhook report --shell bash --shell-pid $$ --terminal $__litemiyu_terminal && printf '\n'
        printf '\e[?25h'
    fi
    __litemiyu_busy=
    # bash 只重画提示符最后一行：前面几行补上。
    [[ $prompt == *$'\n'* ]] && printf '%s\n' "${prompt%$'\n'*}"
    READLINE_LINE=
    [[ -s $leftover ]] && READLINE_LINE=$(<"$leftover")
    READLINE_POINT=${#READLINE_LINE}
    # /edit 放回来的：旁边有一份 .redo 标记，记着「在编辑」，下一次回车替换上一轮重来。
    if [[ -e $leftover.redo ]]; then
        __litemiyu_editing=1
        rm -f -- "$leftover.redo"
    fi
    rm -f -- "$leftover"
}

# 在编辑的那一句：Ctrl+C 清了命令行、执行了别的命令（都会画新的提示符），就不算在编辑了。
__litemiyu_prompt() {
    __litemiyu_editing=
}
PROMPT_COMMAND="__litemiyu_prompt${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# Ctrl+J 在命令行里换行，敲多行的话。
__litemiyu_newline() {
    READLINE_LINE=${READLINE_LINE:0:READLINE_POINT}$'\n'${READLINE_LINE:READLINE_POINT}
    READLINE_POINT=$((READLINE_POINT + 1))
}

# 三套键表都绑：vi 的命令模式按回车也走判断（Esc 以后回车）。
for __litemiyu_map in emacs vi-insert vi-command; do
    bind -m "$__litemiyu_map" -x '"\C-x\C-a": __litemiyu_accept_line'
    bind -m "$__litemiyu_map" '"\C-x\C-b": accept-line'
    bind -m "$__litemiyu_map" '"\C-m": "\C-x\C-a\C-x\C-b"'
done
for __litemiyu_map in emacs vi-insert; do
    bind -m "$__litemiyu_map" -x '"\C-j": __litemiyu_newline'
done
unset __litemiyu_map

# 后台回报：一句话说完以后还有后台任务、循环的，拿着会话的进程留着；它有了新的东西（后台任务结束了、被叫醒开了一轮、
# 在等确认）就给这个 bash 发 SIGUSR1。bash 停在提示符上等你敲的时候不办信号，等你回车、这一行读完、命令还没跑的时候
# 才办；前台在跑命令（vim、AI 正在答）的，等它结束才办（Miyu 实测 bash 5.3）。那时都不在画提示符：直接印。所以停在
# 提示符上来的回报，要等你下一次回车，印在那条命令前面。
__litemiyu_report() {
    # AI 正在答（回车那一下里）：答完再印。
    if [[ -n $__litemiyu_busy ]]; then
        __litemiyu_report_pending=1
        return
    fi
    __litemiyu_shellhook report --shell bash --shell-pid $$ --terminal $__litemiyu_terminal && printf '\n'
    printf '\e[?25h'
}
trap '__litemiyu_report' USR1
