#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    uint64_t nonce;
    uint32_t generation;
    uint32_t timeout_ms;
    volatile LONG result;
    DWORD ui_thread;
    LONG ready;
    LONG licenses;
    LONG database_refreshed;
    LONG reserved;
} ReloadRequest;

static HMODULE module;
static HHOOK hook;
static ReloadRequest *request;
static UINT message;
static HWND window;
static DWORD window_thread;
static LONG completed;
static BOOL ambiguous_thread;

static BOOL executable_range(const void *address, SIZE_T size) {
    MEMORY_BASIC_INFORMATION m;
    if (!VirtualQuery(address, &m, sizeof m) || m.State != MEM_COMMIT || (m.Protect & PAGE_GUARD)) return FALSE;
    DWORD p = m.Protect & 0xff;
    if (p != PAGE_EXECUTE_READ && p != PAGE_EXECUTE_READWRITE && p != PAGE_EXECUTE_WRITECOPY) return FALSE;
    return (uintptr_t)address >= (uintptr_t)m.BaseAddress && size <= m.RegionSize &&
        (uintptr_t)address - (uintptr_t)m.BaseAddress <= m.RegionSize - size;
}

static BOOL memory_range(const void *address, SIZE_T size, BOOL writable) {
    MEMORY_BASIC_INFORMATION m;
    if (!VirtualQuery(address, &m, sizeof m) || m.State != MEM_COMMIT || (m.Protect & PAGE_GUARD)) return FALSE;
    DWORD p = m.Protect & 0xff;
    if (writable && p != PAGE_READWRITE && p != PAGE_WRITECOPY && p != PAGE_EXECUTE_READWRITE && p != PAGE_EXECUTE_WRITECOPY) return FALSE;
    if (!writable && (p == PAGE_NOACCESS || p == PAGE_EXECUTE)) return FALSE;
    return (uintptr_t)address >= (uintptr_t)m.BaseAddress && size <= m.RegionSize &&
        (uintptr_t)address - (uintptr_t)m.BaseAddress <= m.RegionSize - size;
}

static BOOL CALLBACK find_window(HWND hwnd, LPARAM unused) {
    DWORD pid = 0;
    DWORD thread = GetWindowThreadProcessId(hwnd, &pid);
    WCHAR name[64];
    (void)unused;
    if (pid != GetCurrentProcessId() || !GetClassNameW(hwnd, name, 64) || wcsncmp(name, L"JUCE_", 5) != 0)
        return TRUE;
    if (window_thread && window_thread != thread) { ambiguous_thread = TRUE; return FALSE; }
    if (!window || IsWindowVisible(hwnd)) { window = hwnd; window_thread = thread; }
    return TRUE;
}

static LRESULT CALLBACK on_message(int code, WPARAM wp, LPARAM lp) {
    CWPSTRUCT *m = (CWPSTRUCT *)lp;
    (void)wp;
    if (code >= 0 && m && m->message == message && request &&
        (uint64_t)m->wParam == request->nonce && (uint32_t)m->lParam == request->generation &&
        InterlockedCompareExchange(&completed, 1, 0) == 0) {
        unsigned char *base = (unsigned char *)GetModuleHandleW(NULL);
        unsigned char *manager = memory_range(base + 0x17f6a88, sizeof(void *), FALSE)
            ? *(unsigned char **)(base + 0x17f6a88) : NULL;
        if (!manager || !memory_range(manager, 0xb8, TRUE) ||
            !memory_range(base + 0x17f6758, sizeof(void *), FALSE)) { request->result = 2; }
        else {
            int64_t old_cache = *(int64_t *)(manager + 0x68);
            unsigned char old_ready = *(unsigned char *)(manager + 0x58);
            *(int64_t *)(manager + 0x68) = 0;
            *(unsigned char *)(manager + 0x58) = 0;
            unsigned char ok = ((unsigned char (*)(void *, void *, void *))(base + 0x9054b0))(
                manager + 0x68, manager + 0x30, manager + 0xa8);
            if (!ok || !*(unsigned char *)(manager + 0x58)) { *(int64_t *)(manager + 0x68) = old_cache; *(unsigned char *)(manager + 0x58) = old_ready; request->result = 3; }
            else {
                request->ready = *(unsigned char *)(manager + 0x58);
                request->licenses = *(LONG *)(manager + 0xb4);
                unsigned char *db = *(unsigned char **)(base + 0x17f6758);
                if (!db) {
                    db = ((unsigned char *(*)(void))(base + 0x856cf0))();
                    if (db) request->database_refreshed = 1;
                } else {
                    ((void (*)(void *))(base + 0x8c8730))(db);
                    request->database_refreshed = 1;
                }
                request->result = db && request->ready ? 1 : 4;
            }
        }
    }
    return CallNextHookEx(hook, code, wp, lp);
}

__declspec(dllexport) DWORD WINAPI BridgeReload(void *arg) {
    ReloadRequest *r = (ReloadRequest *)arg;
    unsigned char *base = (unsigned char *)GetModuleHandleW(NULL);
    static const unsigned char marker[] = {0xe8,0xe8,0x04,0x00,0x00};
    static const unsigned char reader[] = {0x48,0x89,0x5c,0x24,0x20,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57};
    static const unsigned char refresh[] = {0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x48,0x89,0x7c,0x24,0x20,0x55};
    static const unsigned char getter[] = {0x48,0x89,0x5c,0x24,0x10,0x57,0x48,0x83,0xec,0x30,0x48,0x8b,0x05,0x57,0xfa,0xf9};
    if (!r || !base || !executable_range(base + 0xbcfc3, sizeof marker) ||
        !executable_range(base + 0x9054b0, sizeof reader) ||
        !executable_range(base + 0x8c8730, sizeof refresh) ||
        !executable_range(base + 0x856cf0, sizeof getter) ||
        memcmp(base + 0xbcfc3, marker, sizeof marker) ||
        memcmp(base + 0x9054b0, reader, sizeof reader) ||
        memcmp(base + 0x8c8730, refresh, sizeof refresh) ||
        memcmp(base + 0x856cf0, getter, sizeof getter)) return 10;
    window = NULL; window_thread = 0; ambiguous_thread = FALSE; completed = 0; request = r;
    EnumWindows(find_window, 0);
    if (!window || !window_thread || ambiguous_thread) return 11;
    r->ui_thread = window_thread;
    message = RegisterWindowMessageW(L"Boxy.NativeSession.Reload");
    if (!message) return 12;
    hook = SetWindowsHookExW(WH_CALLWNDPROC, on_message, module, window_thread);
    if (!hook) return 13;
    DWORD_PTR ignored = 0;
    LRESULT sent = SendMessageTimeoutW(window, message, (WPARAM)r->nonce, (LPARAM)r->generation,
                                       SMTO_ABORTIFHUNG | SMTO_BLOCK, r->timeout_ms, &ignored);
    if (!sent) { UnhookWindowsHookEx(hook); hook = NULL; return 14; }
    UnhookWindowsHookEx(hook); hook = NULL;
    return completed ? 0 : 15;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) { module = instance; DisableThreadLibraryCalls(instance); }
    return TRUE;
}
