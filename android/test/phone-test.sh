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
    # An "isn't responding" or error dialog is a window of its own (the launcher may report the
    # focus while the app has it, so the focus is no guide).
    focus=$(adb shell dumpsys window windows | grep -m1 -E "Application (Not Responding|Error)")
    [ -z "$focus" ] && return 0
    echo "system dialog: $focus"
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
# The app logs button positions and the document state only with this property set.
adb shell setprop debug.wordcraft.autotest 1
adb install -r "$APK"
adb shell cmd uimode night no
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 0
adb logcat -c
adb shell am start -W -n "$PKG/.WordActivity"
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
# Whether the on-screen keyboard is up (a real input connection exists and the keyboard shows).
{ adb shell dumpsys input_method | grep -E "mInputShown|mShowRequested|isInputViewShown|mCurId" | head -8; } > "$OUT/keyboard-state.txt" 2>&1
cat "$OUT/keyboard-state.txt"

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

# What a keyboard does through the input connection (compose, commit, delete), played by the
# app's test receiver: swipe typing and autocorrection arrive as exactly these operations.
ime() { adb shell am broadcast -a ai.storyteller.wordcraft.android.IME_TEST "$@" >/dev/null 2>&1; sleep 1.5; }
ime --es op commit --es text "."
ime --es op compose --es text "Ke"
ime --es op compose --es text "Keyb"
shot ime-composing
ime --es op commit --es text "Keyboard."
ime --es op compose --es text "Wischen"
ime --es op finish
ime --es op delete --ei before 2 --ei after 0
# The keyboard takes the word before the caret back into composition and replaces it (a suggestion).
ime --es op word
ime --es op compose --es text "Wischer"
ime --es op finish
sleep 2
shot ime-committed
{
    echo "== keyboard operations the app received"; adb logcat -d -s WordCraft:I | grep "autotest ime"
    echo "== document after them"; doc
    echo "== keyboard shown"; adb shell dumpsys input_method | grep -E "mInputShown|isInputViewShown" | head -3
} > "$OUT/ime-test.txt"
cat "$OUT/ime-test.txt"

tap more
sleep 4
shot all-commands
tap phoneView
sleep 4
shot back-to-phone

tap file
sleep 4
shot file-page
# The phone's File page: the menu fills the screen; a page opens full width with a Back to the
# menu (rows are drawn, not logged: positions for the fixed 720x1600 screen of this test).
adb shell input tap 150 919
sleep 3
shot options-page
adb shell input tap 190 311
sleep 3
shot options-display-pane
adb shell input keyevent 4   # the system Back: page to menu
sleep 3
shot back-to-file-menu
# Home, New and Open: each page opens full width.
adb shell input tap 150 184
sleep 3
shot page-home
adb shell input keyevent 4   # the system Back: page to menu
sleep 3
adb shell input tap 150 258
sleep 3
shot page-new
adb shell input keyevent 4   # the system Back: page to menu
sleep 3
adb shell input tap 150 331
sleep 3
shot page-open
adb shell input keyevent 4   # the system Back: page to menu
sleep 3
adb shell input keyevent 111
sleep 4
shot after-file-page

adb shell cmd uimode night yes
sleep 6
shot night-mode
# A tap on the top bar's empty middle, in case the change only shows with the next input.
read -r FX FY < <(button file)
adb shell input tap $((W / 2)) "${FY:-130}"
sleep 3
shot night-mode-after-tap

adb shell wm user-rotation lock 1 || adb shell settings put system user_rotation 1
sleep 8
shot landscape
adb shell wm user-rotation lock 0 || adb shell settings put system user_rotation 0
sleep 4

# A restart of the activity that no app can opt out of (an overlay changes every app's
# resources): the app must not freeze ("isn't responding"); it ends and Android starts it again.
pid_before=$(adb shell pidof "$PKG")
adb shell cmd overlay enable com.android.internal.display.cutout.emulation.corner
sleep 15
pid_after=$(adb shell pidof "$PKG")
shot after-activity-restart
adb shell cmd overlay disable com.android.internal.display.cutout.emulation.corner
sleep 10
{
    echo "process before the restart: ${pid_before:-none}, after: ${pid_after:-none}"
    adb logcat -d | grep -E "ANR in $PKG" || echo "no ANR"
} > "$OUT/activity-restart.txt"
cat "$OUT/activity-restart.txt"

adb pull /data/anr "$OUT/anr" >/dev/null 2>&1 || true
# The document after each step, as the app logged it (text and formatting at the caret).
adb logcat -d -s WordCraft:I | grep "autotest doc" > "$OUT/doc-states.txt" || true
adb logcat -d -s WordCraft:* > "$OUT/logcat-wordcraft.txt"
adb logcat -d -b crash > "$OUT/logcat-crash.txt" || true
adb logcat -d > "$OUT/logcat-all.txt"
echo "done: $n screenshots"
