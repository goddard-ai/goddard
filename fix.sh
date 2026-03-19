awk '
/from "@goddard-ai\/schema\/workforce"/ {
    print
    getline
    if ($0 == "") {
        # Skip the blank line
    } else {
        print
    }
    next
}
{ print }
' core/daemon/src/workforce/runtime.ts > temp.ts
mv temp.ts core/daemon/src/workforce/runtime.ts
