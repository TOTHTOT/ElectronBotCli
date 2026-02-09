/**
 * ElectronBot USB Test Program (Simplified)
 *
 * Build: gcc -o ele_test ele_test.c -lusb-1.0
 * Run: sudo ./ele_test
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <libusb-1.0/libusb.h>

/* USB Device Parameters */
#define VID             0x1001
#define PID             0x8023
#define EP_OUT          0x01
#define TIMEOUT         1000

/* LCD Parameters */
#define LCD_WIDTH       240
#define LCD_HEIGHT      240
#define BYTES_PER_PIXEL 3
#define ROW_SIZE        (LCD_WIDTH * BYTES_PER_PIXEL)
#define ROWS_PER_ROUND  60
#define BYTES_PER_ROUND (ROWS_PER_ROUND * ROW_SIZE)
#define ROUND_COUNT     4
#define USB_PACKET_SIZE 512
#define PACKETS_PER_ROUND (BYTES_PER_ROUND / USB_PACKET_SIZE)
#define TAIL_SIZE       224
#define PIXELS_IN_TAIL  192

/**
 * 发送像素数据（分包发送）
 */
int send_pixels(libusb_device_handle *handle, const unsigned char *pixels, int round)
{
    int transferred;
    size_t start = round * BYTES_PER_ROUND;

    for (int i = 0; i < PACKETS_PER_ROUND; i++) {
        size_t offset = start + i * USB_PACKET_SIZE;
        int ret = libusb_bulk_transfer(handle, EP_OUT,
                                       (unsigned char *)&pixels[offset],
                                       USB_PACKET_SIZE, &transferred, TIMEOUT);
        if (ret != 0) {
            fprintf(stderr, "Bulk write failed at packet %d: %s\n", i,
                    libusb_error_name(ret));
            return -1;
        }
    }
    return 0;
}

/**
 * 发送尾部数据
 */
int send_tail(libusb_device_handle *handle, const unsigned char *pixels,
              const unsigned char *joint_config, int round)
{
    int transferred;
    unsigned char tail[TAIL_SIZE];
    size_t tail_offset = round * BYTES_PER_ROUND + BYTES_PER_ROUND - PIXELS_IN_TAIL;

    memset(tail, 0xFF, TAIL_SIZE);
    memcpy(tail, &pixels[tail_offset], PIXELS_IN_TAIL);
    memcpy(tail + PIXELS_IN_TAIL, joint_config, 32);

    int ret = libusb_bulk_transfer(handle, EP_OUT, tail, TAIL_SIZE,
                                   &transferred, TIMEOUT);
    if (ret != 0) {
        fprintf(stderr, "Tail write failed: %s\n", libusb_error_name(ret));
        return -1;
    }
    return 0;
}

/**
 * 生成测试图案（水平条纹）
 */
void generate_test_pattern(unsigned char *pixels)
{
    memset(pixels, 0, LCD_WIDTH * LCD_HEIGHT * BYTES_PER_PIXEL);

    /* 每行不同颜色 */
    for (int y = 0; y < LCD_HEIGHT; y++) {
        unsigned char color[3] = {
            (y % 256),
            ((y * 2) % 256),
            ((y * 3) % 256)
        };
        for (int x = 0; x < LCD_WIDTH; x++) {
            int idx = (y * LCD_WIDTH + x) * 3;
            pixels[idx] = color[0];
            pixels[idx + 1] = color[1];
            pixels[idx + 2] = color[2];
        }
    }
}

int main(int argc, char *argv[])
{
    libusb_context *ctx = NULL;
    libusb_device_handle *handle = NULL;
    libusb_device **devices;
    struct libusb_device_descriptor desc;
    ssize_t count;
    int ret = -1;
    unsigned char *pixels = NULL;
    unsigned char joint_config[32] = {0};

    printf("ElectronBot USB Test\n\n");

    ret = libusb_init(&ctx);
    if (ret < 0) {
        fprintf(stderr, "libusb_init failed\n");
        return 1;
    }

    count = libusb_get_device_list(ctx, &devices);
    if (count < 0) goto cleanup;

    printf("Searching for device %04X:%04X...\n", VID, PID);
    for (ssize_t i = 0; i < count; i++) {
        libusb_device *dev = devices[i];
        ret = libusb_get_device_descriptor(dev, &desc);
        if (ret < 0) continue;

        if (desc.idVendor == VID && desc.idProduct == PID) {
            printf("Device found!\n");

            ret = libusb_open(dev, &handle);
            if (ret < 0) goto cleanup;

            if (libusb_kernel_driver_active(handle, 0) == 1) {
                libusb_detach_kernel_driver(handle, 0);
            }

            ret = libusb_claim_interface(handle, 0);
            if (ret < 0) goto cleanup;
            printf("Interface claimed\n");
            break;
        }
    }

    if (!handle) {
        fprintf(stderr, "Device not found!\n");
        goto cleanup;
    }

    pixels = malloc(LCD_WIDTH * LCD_HEIGHT * BYTES_PER_PIXEL);
    if (!pixels) goto cleanup;

    generate_test_pattern(pixels);

    printf("\nSending frame (4 rounds)...\n");
    for (int round = 0; round < ROUND_COUNT; round++) {
        printf("Round %d...\n", round);

        if (send_pixels(handle, pixels, round) < 0) goto cleanup;
        if (send_tail(handle, pixels, joint_config, round) < 0) goto cleanup;

        usleep(1000);  /* 1ms delay */
    }

    printf("\nDone!\n");
    ret = 0;

cleanup:
    if (handle) {
        libusb_release_interface(handle, 0);
        libusb_close(handle);
    }
    if (devices) libusb_free_device_list(devices, 1);
    if (pixels) free(pixels);
    if (ctx) libusb_exit(ctx);

    return ret;
}
