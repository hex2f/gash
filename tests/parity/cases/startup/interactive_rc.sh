# parity: home=home-bashrc-only

bash -i -c 'echo "bashrc: ${HOME_RC_MARK-unset}"' 2>/dev/null
printf 'export RCFILE_MARK=rcfile\n' > rc
bash --rcfile rc -i -c 'echo "rcfile: ${RCFILE_MARK-unset} ${HOME_RC_MARK-unset}"' 2>/dev/null
