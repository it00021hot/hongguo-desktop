package com.hongguo.nsh;

/** The native host's event ABI; the production adapter delegates to NshComponentContext. */
public interface NativeEventSink {
    void emit(String name, Object value);
}
