#!/usr/bin/env bash
# Emulator test of the phone layout (run by .github/workflows/android.yml in the emulator job).
# Plays real touch and key input through adb and saves a screenshot after every step, so the
# result can be looked at without a phone: android/test/phone-test.sh <apk> <output folder>.
# The app logs each phone button's centre in pixels ("autotest button <name> <x> <y>"), which is
# how the script finds what to tap.
set -u
APK=$1
OUT=$2
PKG=ai.storyteller.wordcraft.android
mkdir -p "$OUT"
n=0
shot() { n=$((n + 1)); adb exec-out screencap -p > "$OUT/$(printf %02d $n)-$1.png"; echo "shot $n: $1"; }
# The last logged centre of a phone button, as "x y" (empty if it has not been drawn).
button() { adb logcat -d -s WordCraft:I | grep "autotest button $1 " | tail -1 | awk '{print $(NF-1), $NF}'; }
tap() {
    local xy
    xy=$(button "$1")
    if [ -z "$xy" ]; then echo "no button $1"; return 1; fi
    echo "tap $1 at $xy"
    adb shell input tap $xy
}
type_text() { adb shell input text "$(printf %s "$1" | sed 's/ /%s/g')"; }

adb install -r "$APK"
adb shell cmd uimode night no
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 0
adb logcat -c
adb shell am start -W -n "$PKG/android.app.NativeActivity"
sleep 20
shot start-page

# Escape closes the File page (the start page) and shows the blank document.
adb shell input keyevent 111
sleep 5
shot phone-editor

# Tap into the page: the caret is placed and the on-screen keyboard opens.
read -r W H < <(adb shell wm size | tail -1 | sed -E 's/.*: ([0-9]+)x([0-9]+).*/\1 \2/')
adb shell input tap $((W / 2)) $((H / 5))
sleep 4
shot keyboard-open

type_text "Hallo "
sleep 2
tap bold
sleep 3
shot after-bold-tap
type_text "fett"
sleep 2
shot typed-after-bold
tap bold
sleep 2
type_text " normal "
tap italic
sleep 2
type_text "kursiv"
sleep 2
shot bold-off-italic-on

tap highlight
sleep 3
shot highlight-menu
tap yellow
sleep 2
type_text " markiert"
sleep 2
shot after-highlight

tap fontcolor
sleep 3
shot color-menu
tap FF0000
sleep 2
type_text " rot"
sleep 2
shot after-color

tap addShape
sleep 3
shot insert-menu
tap addShape
sleep 2

tap undo
sleep 2
shot after-undo

tap toolbarMore
sleep 4
shot all-commands
tap phoneView
sleep 4
shot back-to-phone

tap file
sleep 4
shot file-page
adb shell input keyevent 111
sleep 4
shot after-file-page

adb shell cmd uimode night yes
sleep 6
shot night-mode

adb shell settings put system user_rotation 1
sleep 6
shot landscape
adb shell settings put system user_rotation 0
sleep 4

adb logcat -d -s WordCraft:* > "$OUT/logcat-wordcraft.txt"
adb logcat -d -b crash > "$OUT/logcat-crash.txt" || true
adb logcat -d > "$OUT/logcat-all.txt"
echo "done: $n screenshots"
