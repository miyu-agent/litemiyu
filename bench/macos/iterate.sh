#!/bin/sh
# 一轮轮补清单：带页探针跑（空闲、真 HTTPS 第二次起），把碰到的清单外的函数接到清单后面，重链带符号的那一份，再跑。
# 每一轮的清单换一个文件名：同一个文件名 cargo 认为参数没变、不重链。
. ~/litemiyu-lab/scripts/env.sh
cd $LAB
awk '!seen[$0]++' tmp/order-iter.txt > tmp/order-r0.txt
rm -f tmp/iterate.log
relink() {
  # 删掉上一份：光换清单的文件名 cargo 有时照旧认为不用重链（2026-11-22 碰到）。
  rm -f target-sym/release/litemiyu
  for f in target-sym/release/deps/litemiyu-*; do case "$f" in *.d) ;; *) rm -f "$f";; esac; done
  (cd src && CARGO_PROFILE_RELEASE_STRIP=false CARGO_TARGET_DIR=$LAB/target-sym cargo rustc --release -p litemiyu --bin litemiyu -- -C link-arg=-Wl,-order_file,$LAB/tmp/order-r$1.txt > $LAB/tmp/build-iter.log 2>&1)
}
relink 0 || { echo "重链失败" >> tmp/iterate.log; exit 1; }
round=0
while [ $round -lt 12 ]; do
  LANG=zh_CN.UTF-8 python3 scripts/tracerun.py target-sym/release/litemiyu bin/pagetrace.dylib idle tmp/order-r$round.txt tmp/trace-idle.txt > tmp/iter-idle.out 2>&1
  LANG=zh_CN.UTF-8 python3 scripts/tracerun.py target-sym/release/litemiyu bin/pagetrace.dylib real tmp/order-r$round.txt tmp/trace-real.txt > tmp/iter-real.out 2>&1
  new=$(cat tmp/trace-idle.txt tmp/trace-real.txt | grep "清单外" | cut -f3 | sort -u | wc -l | tr -d " ")
  echo "第 $round 轮（清单 $(wc -l < tmp/order-r$round.txt | tr -d ' ') 个）：$(head -1 tmp/iter-idle.out)；$(head -1 tmp/iter-real.out)；清单外的函数 $new 个" >> tmp/iterate.log
  [ "$new" -lt 4 ] && break
  next=$((round + 1))
  (cat tmp/order-r$round.txt; grep "清单外" tmp/trace-idle.txt | cut -f3; grep "清单外" tmp/trace-real.txt | cut -f3) | awk '!seen[$0]++' > tmp/order-r$next.txt
  relink $next || { echo "第 $next 轮重链失败" >> tmp/iterate.log; break; }
  round=$next
done
cp tmp/order-r$round.txt tmp/order-final.txt
echo "完" >> tmp/iterate.log
