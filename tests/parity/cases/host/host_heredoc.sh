# parity: mode=host

cat <<'EOF'
line one
  indented $HOME 'single' "double"
EOF
x=3
cat <<EOF
expanded x=$x
EOF
echo "after heredocs"
