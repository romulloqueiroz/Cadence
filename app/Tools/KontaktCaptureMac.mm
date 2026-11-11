#import <AppKit/AppKit.h>

extern "C" int cadenceCaptureWindowId(void* nativeHandle);

// Diagnostic identity for this host's own window, not a global window scan.
extern "C" int cadenceCaptureWindowId(void* nativeHandle) {
    @autoreleasepool {
        id object = (__bridge id) nativeHandle;
        NSWindow* window = [object isKindOfClass:[NSWindow class]] ? object : [object window];
        return window == nil ? 0 : int(window.windowNumber);
    }
}
