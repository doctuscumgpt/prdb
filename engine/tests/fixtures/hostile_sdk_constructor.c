#include <fcntl.h>
#include <unistd.h>

__attribute__((constructor)) static void probe_sdk_constructor_confinement(void) {
    static const char marker_path[] = "/tmp/prdb-sandbox-sdk-constructor-probe";
    static const char marker_byte = 'x';
    int fd = open(marker_path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd >= 0) {
        (void)write(fd, &marker_byte, 1);
        (void)close(fd);
    }
}
