# A script that starts with `exec > >(tee log) 2>&1` (or `&>`, `>&`, `&>>` into `>(tee log)`)
# writes its output to both its stdout and the log when a nested shell runs it into a file.
t=$(mktemp -d)
cd "$t" || exit 1

# Git Bash does not wait for a process substitution at exit, so tee can still be writing.
settle() {
  i=0
  while [ $i -lt 15 ] && ! { grep -qx end out 2>/dev/null && grep -qx end log 2>/dev/null; }; do
    sleep 0.1
    i=$((i + 1))
  done
}

run() {
  echo "-- exec $1"
  rm -f out log
  printf 'set -euo pipefail\nexec %s\necho hello\necho err >&2\nls -d . > /dev/null\necho end\n' "$1" > s.sh
  "$BASH" s.sh > out 2>&1
  echo "rc=$?"
  settle
  echo "out:"
  cat out
  echo "log:"
  cat log
}

run '> >(tee log) 2>&1'
run '&> >(tee log)'
run '>& >(tee log)'
run '&>> >(tee log)'

# `1>&` into a process substitution takes stdout and stderr, like `>&`.
echo "-- 1>& >(tee f)"
{ { echo y; echo e >&2; } 1>& >(tee f > /dev/null); echo back >&2; } 2>&1
echo "rc=$?"
i=0
while [ $i -lt 15 ] && ! grep -qx e f 2>/dev/null; do
  sleep 0.1
  i=$((i + 1))
done
cat f

cd / && rm -rf "$t"
