/**
 * ElectronBot USB Test - Minimal Version
 *
 * Uses libusb-1.0 directly
 * Based on USBInterface.cpp
 *
 * Build: gcc -o ele_test minimal_test.c -lusb-1.0 -I/usr/include/libusb-1.0
 * Run: sudo ./ele_test
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <libusb-1.0/libusb.h>

/* USB Parameters */
#define VID             0x1001
#define PID             0x8023
#define TIMEOUT         1000

/* Try different endpoint configurations */
/* Option 1: 0x01 (same as Windows) */
/* Option 2: 0x02 (endpoint 2) */
/* Option 3: 0x04 (endpoint 4) */
#define EP_OUT          0x02  /* Try endpoint 2 */

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

int main(int argc, char *argv[])
{
    libusb_context *ctx = NULL;
    libusb_device_handle *handle = NULL;
    libusb_device **devices = NULL;
    struct libusb_device_descriptor desc;
    ssize_t count;
    int ret = -1;
    unsigned char *pixels = NULL;
    unsigned char joint_config[32] = {0};

    printf("=== Minimal USB Test ===\n\n");

    /* Initialize libusb */
    ret = libusb_init(&ctx);
    if (ret < 0) {
        fprintf(stderr, "libusb_init failed: %s\n", libusb_error_name(ret));
        return 1;
    }

    /* Get device list */
    count = libusb_get_device_list(ctx, &devices);
    if (count < 0) {
        fprintf(stderr, "libusb_get_device_list failed\n");
        goto cleanup;
    }

    /* Find device */
    printf("Looking for %04X:%04X...\n", VID, PID);
    for (ssize_t i = 0; i < count; i++) {
        libusb_device *dev = devices[i];
        ret = libusb_get_device_descriptor(dev, &desc);
        if (ret < 0) continue;

        if (desc.idVendor == VID && desc.idProduct == PID) {
            printf("Found device!\n");

            ret = libusb_open(dev, &handle);
            if (ret < 0) {
                fprintf(stderr, "libusb_open failed: %s\n", libusb_error_name(ret));
                goto cleanup;
            }

            /* Detach kernel driver if active */
            if (libusb_kernel_driver_active(handle, 1) == 1) {
                ret = libusb_detach_kernel_driver(handle, 1);
                if (ret < 0) {
                    fprintf(stderr, "detach_kernel_driver failed: %s\n", libusb_error_name(ret));
                } else {
                    printf("Kernel driver detached\n");
                }
            }

            /* Try interface 1 (like Windows code) */
            ret = libusb_claim_interface(handle, 1);
            if (ret < 0) {
                fprintf(stderr, "claim_interface failed: %s\n", libusb_error_name(ret));
                goto cleanup;
            }
            printf("Interface claimed\n");
            break;
        }
    }

    if (!handle) {
        fprintf(stderr, "Device not found!\n");
        goto cleanup;
    }

    /* Allocate pixel buffer */
    pixels = malloc(LCD_WIDTH * LCD_HEIGHT * BYTES_PER_PIXEL);
    if (!pixels) {
        fprintf(stderr, "malloc failed\n");
        goto cleanup;
    }

    /* Generate test pattern - red gradient */
    printf("Generating test pattern...\n");
    for (int y = 0; y < LCD_HEIGHT; y++) {
        for (int x = 0; x < LCD_WIDTH; x++) {
            int idx = (y * LCD_WIDTH + x) * 3;
            pixels[idx] = (y * 256 / LCD_HEIGHT);     /* R - gradient */
            pixels[idx + 1] = 0;                       /* G */
            pixels[idx + 2] = 0;                       /* B */
        }
    }

    /* Send 4 rounds of data */
    printf("\nSending data...\n");

    for (int round = 0; round < ROUND_COUNT; round++) {
        int transferred;
        size_t start = round * BYTES_PER_ROUND;

        printf("Round %d: ", round);

        /* Send 84 packets of 512 bytes each */
        for (int i = 0; i < PACKETS_PER_ROUND; i++) {
            size_t offset = start + i * USB_PACKET_SIZE;

            ret = libusb_bulk_transfer(handle, EP_OUT,
                                     &pixels[offset],
                                     USB_PACKET_SIZE,
                                     &transferred,
                                     TIMEOUT);
            if (ret < 0) {
                fprintf(stderr, "bulk_write failed at packet %d: %s\n",
                        i, libusb_error_name(ret));
                goto cleanup;
            }
        }

        /* Send tail (192 pixels + 32 bytes config = 224 bytes) */
        unsigned char tail[TAIL_SIZE];
        size_t tail_offset = round * BYTES_PER_ROUND + BYTES_PER_ROUND - PIXELS_IN_TAIL;

        memset(tail, 0xFF, TAIL_SIZE);
        memcpy(tail, &pixels[tail_offset], PIXELS_IN_TAIL);
        memcpy(tail + PIXELS_IN_TAIL, joint_config, 32);

        ret = libusb_bulk_transfer(handle, EP_OUT, tail, TAIL_SIZE,
                                   &transferred, TIMEOUT);
        if (ret < 0) {
            fprintf(stderr, "tail_write failed: %s\n", libusb_error_name(ret));
            goto cleanup;
        }

        printf("OK (%d + %d bytes)\n", PACKETS_PER_ROUND * USB_PACKET_SIZE, TAIL_SIZE);

        /* Small delay between rounds */
        usleep(1000);
    }

    printf("\n=== Test completed successfully! ===\n");
    ret = 0;

cleanup:
    if (handle) {
        libusb_release_interface(handle, 1);
        libusb_close(handle);
    }
    if (devices) {
        libusb_free_device_list(devices, 1);
    }
    if (pixels) {
        free(pixels);
    }
    if (ctx) {
        libusb_exit(ctx);
    }

    return ret;
}
