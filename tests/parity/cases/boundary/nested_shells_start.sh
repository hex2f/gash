# parity: mode=c
# parity: serial=1

bash -c 'echo "bash: BASH=$BASH"; echo "bash: v${BASH_VERSINFO[0]} SHLVL=$SHLVL"'
sh -c 'echo "sh: BASH=$BASH"; echo "sh: SHLVL=$SHLVL"'
env bash -c 'echo "env bash: BASH=$BASH"; echo "env bash: SHLVL=$SHLVL"'
echo "outer: SHLVL=$SHLVL"
