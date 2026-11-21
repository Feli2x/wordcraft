package ai.storyteller.wordcraft.android;

import android.app.NativeActivity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.os.Build;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.text.TextUtils;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * The NativeActivity with a real text-input connection.
 *
 * A plain NativeActivity gives the soft keyboard no InputConnection, so the keyboard sends bare key
 * events: no swipe typing, no suggestions, no autocorrection, no automatic capital letters. This
 * activity adds a one-pixel view that is the window's text editor. Its InputConnection mirrors the
 * text around WordCraft's caret (reported by the Rust side through {@link #imeState}) and turns
 * what the keyboard does (compose, commit, delete) into operations the Rust side takes with
 * {@link #imeDrain} and hands to WordCraft's own input-method code.
 */
public class WordActivity extends NativeActivity {
    static {
        System.loadLibrary("wordcraft_android");
    }

    /** Wakes the Rust event loop (a keyboard operation is waiting). */
    private static native void nativeWake();

    static void wake() {
        try {
            nativeWake();
        } catch (Throwable ignored) {
            // The Rust side also polls while the keyboard is shown.
        }
    }

    private ImeView ime;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        ime = new ImeView(this);
        addContentView(ime, new ViewGroup.LayoutParams(1, 1));
        ime.requestFocus();
    }

    /** Operations the keyboard made since the last call, encoded (see ImeView.push); "" if none. */
    public String imeDrain() {
        return ime == null ? "" : ime.drain();
    }

    /**
     * WordCraft's text around the caret: UTF-16 offsets of the caret and the selection's anchor,
     * and the number of the last operation the Rust side has applied.
     */
    public void imeState(String text, int cursor, int anchor, int ack) {
        if (ime != null) ime.setState(text, cursor, anchor, ack);
    }

    /** Show or hide the soft keyboard. */
    public void imeVisible(final boolean show) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (ime == null) return;
                InputMethodManager imm = (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
                if (show) {
                    ime.requestFocus();
                    imm.showSoftInput(ime, 0);
                } else {
                    imm.hideSoftInputFromWindow(ime.getWindowToken(), 0);
                }
            }
        });
    }

    /**
     * For the emulator test only (the Rust side calls it when autotest is on): lets `am broadcast
     * -a ai.storyteller.wordcraft.android.IME_TEST --es op compose --es text Hal` play what a
     * keyboard does, through the same connection.
     */
    public void imeEnableTest() {
        IntentFilter filter = new IntentFilter("ai.storyteller.wordcraft.android.IME_TEST");
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                if (ime == null) return;
                String op = intent.getStringExtra("op");
                String text = intent.getStringExtra("text");
                InputConnection c = ime.onCreateInputConnection(new EditorInfo());
                if ("compose".equals(op)) c.setComposingText(text, 1);
                else if ("commit".equals(op)) c.commitText(text, 1);
                else if ("finish".equals(op)) c.finishComposingText();
                else if ("delete".equals(op)) c.deleteSurroundingText(intent.getIntExtra("before", 1), intent.getIntExtra("after", 0));
                else if ("region".equals(op)) c.setComposingRegion(intent.getIntExtra("start", 0), intent.getIntExtra("end", 0));
            }
        };
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED);
        else registerReceiver(receiver, filter);
    }

    /** The window's text editor: invisible, always focused. */
    static final class ImeView extends View {
        static final int PREEDIT = 0, COMMIT = 1, DELETE = 2;

        final Object lock = new Object();
        /** The committed text around the caret with the composing text in it, as the keyboard sees it. */
        final SpannableStringBuilder buf = new SpannableStringBuilder();
        final StringBuilder out = new StringBuilder();
        int seq = 0;

        ImeView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo info) {
            info.inputType = InputType.TYPE_CLASS_TEXT
                    | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                    | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
            // Enter is a new paragraph (a key event), not an action; no full-screen editor.
            info.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                    | EditorInfo.IME_FLAG_NO_ENTER_ACTION | EditorInfo.IME_ACTION_NONE;
            synchronized (lock) {
                info.initialSelStart = Selection.getSelectionStart(buf);
                info.initialSelEnd = Selection.getSelectionEnd(buf);
                info.initialCapsMode = TextUtils.getCapsMode(buf, Math.max(0, info.initialSelStart), info.inputType);
            }
            return new Conn(this);
        }

        /** Queue an operation: kind, number, two counts and text, fields split by \u0001. */
        void push(int kind, String text, int a, int b) {
            seq++;
            if (out.length() > 0) out.append('\u0002');
            out.append(kind).append('\u0001').append(seq).append('\u0001').append(a).append('\u0001').append(b).append('\u0001')
                    .append(text.replace('\u0001', ' ').replace('\u0002', ' '));
            wake();
        }

        String drain() {
            synchronized (lock) {
                String s = out.toString();
                out.setLength(0);
                return s;
            }
        }

        int composingStart() {
            return BaseInputConnection.getComposingSpanStart(buf);
        }

        int composingEnd() {
            return BaseInputConnection.getComposingSpanEnd(buf);
        }

        /** The committed text: the buffer without its composing part. */
        String committed() {
            int s = composingStart(), e = composingEnd();
            if (s < 0 || e < s) return buf.toString();
            return buf.subSequence(0, s).toString() + buf.subSequence(e, buf.length()).toString();
        }

        /** An offset of the buffer as an offset of the committed text. */
        int toCommitted(int off) {
            int s = composingStart(), e = composingEnd();
            if (s < 0 || e < s) return off;
            if (off <= s) return off;
            if (off >= e) return off - (e - s);
            return s;
        }

        void setState(String text, int cursor, int anchor, int ack) {
            boolean restart;
            synchronized (lock) {
                // The keyboard's operations are not all applied yet: the state is older than the buffer.
                if (ack != seq) return;
                int c = toCommitted(Selection.getSelectionEnd(buf));
                int a = toCommitted(Selection.getSelectionStart(buf));
                if (committed().equals(text) && c == cursor && a == anchor) return;
                // The caret moved, or the text changed other than by the keyboard: start over.
                BaseInputConnection.removeComposingSpans(buf);
                buf.replace(0, buf.length(), text);
                Selection.setSelection(buf, Math.min(anchor, text.length()), Math.min(cursor, text.length()));
                restart = true;
            }
            if (restart) {
                post(new Runnable() {
                    @Override
                    public void run() {
                        InputMethodManager imm = (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
                        imm.restartInput(ImeView.this);
                    }
                });
            }
        }
    }

    /** What the keyboard does to the text. */
    static final class Conn extends BaseInputConnection {
        private final ImeView view;

        Conn(ImeView view) {
            super(view, true);
            this.view = view;
        }

        @Override
        public Editable getEditable() {
            return view.buf;
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            synchronized (view.lock) {
                view.push(ImeView.PREEDIT, text.toString(), 0, 0);
                return super.setComposingText(text, newCursorPosition);
            }
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            synchronized (view.lock) {
                view.push(ImeView.COMMIT, text.toString(), 0, 0);
                return super.commitText(text, newCursorPosition);
            }
        }

        @Override
        public boolean finishComposingText() {
            synchronized (view.lock) {
                int s = view.composingStart(), e = view.composingEnd();
                if (s >= 0 && e > s) view.push(ImeView.COMMIT, view.buf.subSequence(s, e).toString(), 0, 0);
                return super.finishComposingText();
            }
        }

        @Override
        public boolean deleteSurroundingText(int before, int after) {
            synchronized (view.lock) {
                String s = view.buf.toString();
                int c = Math.min(Selection.getSelectionStart(view.buf), s.length());
                int se = Math.min(Selection.getSelectionEnd(view.buf), s.length());
                int from = Math.max(0, c - before);
                int to = Math.min(s.length(), se + after);
                view.push(ImeView.DELETE, "", s.codePointCount(from, c), s.codePointCount(se, to));
                return super.deleteSurroundingText(before, after);
            }
        }

        @Override
        public boolean deleteSurroundingTextInCodePoints(int before, int after) {
            synchronized (view.lock) {
                view.push(ImeView.DELETE, "", before, after);
                return super.deleteSurroundingTextInCodePoints(before, after);
            }
        }

        @Override
        public boolean setComposingRegion(int start, int end) {
            synchronized (view.lock) {
                int s = Math.max(0, Math.min(start, end)), e = Math.min(view.buf.length(), Math.max(start, end));
                int c = Selection.getSelectionEnd(view.buf);
                String text = view.buf.toString();
                // The keyboard takes the word at the caret back into composition (a tap on a word,
                // or Backspace after it): WordCraft removes it and shows it as the pre-edit text.
                if (view.composingStart() < 0 && s < e && s <= c && c <= e) {
                    view.push(ImeView.DELETE, "", text.codePointCount(s, c), text.codePointCount(c, e));
                    view.push(ImeView.PREEDIT, text.substring(s, e), 0, 0);
                    return super.setComposingRegion(s, e);
                }
                return true;
            }
        }

        @Override
        public boolean setSelection(int start, int end) {
            // The caret is WordCraft's: the keyboard's cursor moves arrive as key events.
            return true;
        }
    }
}
