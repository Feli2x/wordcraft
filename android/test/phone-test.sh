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
    dismiss_dialogs
    local xy
    xy=$(button "$1")
    if [ -z "$xy" ]; then echo "no button $1"; return 1; fi
    echo "tap $1 at $xy"
    adb shell input tap $xy
}
type_text() { dismiss_dialogs; adb shell input text "$(printf %s "$1" | sed 's/ /%s/g')"; }
# If a system dialog has the focus instead of the app, tap its "Wait" (or "OK") button.
dismiss_dialogs() {
    local focus bounds
    focus=$(adb shell dumpsys window | grep -m1 mCurrentFocus)
    case "$focus" in *"$PKG"*) return 0 ;; esac
    echo "focus not on the app: $focus"
    adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1
    bounds=$(adb shell cat /sdcard/ui.xml 2>/dev/null | grep -oE 'text="(Wait|OK)"[^>]*bounds="[^"]*"' | head -1 | grep -oE 'bounds="[^"]*"' | grep -oE '[0-9]+' | tr '\n' ' ')
    if [ -n "$bounds" ]; then
        set -- $bounds
        echo "dialog answered at $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 ))"
        adb shell input tap $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 ))
        sleep 2
    fi
}

# Diagnostics need root (ANR traces, native stacks); the google_apis emulator allows it.
adb root >/dev/null 2>&1 && sleep 3
# Software rendering (SwiftShader) is slow at 1080x2400: the same 411 dp wide layout at fewer pixels.
adb shell wm size 720x1600
adb shell wm density 280
# A freshly booted emulator keeps the CPU busy for a while (package scans, first-run services).
sleep 45
# Starved by the software renderer, system apps may show "isn't responding" dialogs that take the
# test's keys (run 23); they are hidden where the system allows it and answered "Wait" otherwise.
# (Disabling the launcher instead made the system close the app at once, run 24.)
adb shell settings put global hide_error_dialogs 1
adb install -r "$APK"
adb shell cmd uimode night no
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 0
adb logcat -c
adb shell am start -W -n "$PKG/android.app.NativeActivity"
sleep 20
shot start-page

# What has the focus, and what the app's threads are doing, before the first key.
{
    echo "== focus"; adb shell dumpsys window | grep -E "mCurrentFocus|mFocusedApp"
    echo "== activity"; adb shell dumpsys activity activities | grep -E "ResumedActivity|mFocusedApp" | head -5
    pid=$(adb shell pidof "$PKG")
    echo "== pid $pid threads"; adb shell debuggerd -b "$pid" 2>&1 | head -400
} > "$OUT/state-before-first-key.txt" 2>&1

# The document state the app logged last ("autotest doc ...").
doc() { adb logcat -d -s WordCraft:I | grep "autotest doc" | tail -1 | sed -E 's/.*autotest doc //'; }

# Escape closes the File page (the start page); with no document open the phone editor starts a
# blank one. Tried again if a system dialog took the key.
for attempt in 1 2 3; do
    dismiss_dialogs
    adb shell input keyevent 111
    sleep 5
    case "$(doc)" in *fullScreenView=true*) break ;; esac
    echo "phone editor not shown after Escape (attempt $attempt): $(doc)"
done
shot phone-editor

# Tap into the page, halfway between the top bar and the toolbar: the caret is placed and the
# on-screen keyboard opens.
read -r W H < <(adb shell wm size | tail -1 | sed -E 's/.*: ([0-9]+)x([0-9]+).*/\1 \2/')
read -r _ TOP < <(button file)
read -r _ BOTTOM < <(button bold)
adb shell input tap $((W / 2)) $(( (${TOP:-200} + ${BOTTOM:-1200}) / 3 ))
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

adb pull /data/anr "$OUT/anr" >/dev/null 2>&1 || true
# The document after each step, as the app logged it (text and formatting at the caret).
adb logcat -d -s WordCraft:I | grep "autotest doc" > "$OUT/doc-states.txt" || true
adb logcat -d -s WordCraft:* > "$OUT/logcat-wordcraft.txt"
adb logcat -d -b crash > "$OUT/logcat-crash.txt" || true
adb logcat -d > "$OUT/logcat-all.txt"
echo "done: $n screenshots"
