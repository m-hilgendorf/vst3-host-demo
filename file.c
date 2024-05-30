#include <stdio.h>
#include <sys/utsname.h>
#include <dlfcn.h>

typedef char (*ModuleEntryFunc)(void*);

int main() {
    const char* path = "/home/mikedorf/dev/vst3sdk/build/VST3/Debug/adelay.vst3/Contents/x86_64-linux/adelay.so";
    void* lib = dlopen(path, RTLD_LAZY);
    if(!lib) {
        perror("dlopen");
        return 1;
    }

    ModuleEntryFunc entryFunc;
    entryFunc = dlsym(lib, "ModuleEntry");
    if (!entryFunc) {
        perror("dlsym");
        dlclose(lib);
        return 1;
    }

    if(!entryFunc(lib)) {
        printf("failed to init\n");
        return 1;
    }

    if (dlclose(lib)) {
        perror("dlclose");
        return 1;
    }

    return 0;
}