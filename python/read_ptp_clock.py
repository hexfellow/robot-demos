import os
import ctypes
import time
from ctypes import c_int, c_long, POINTER, Structure
import asyncio

# Define the timespec structure
class Timespec(Structure):
    _fields_ = [("tv_sec", c_long), ("tv_nsec", c_long)]

# Load the C standard library
libc = ctypes.CDLL("libc.so.6", use_errno=True)

# Define the clock_gettime function
libc.clock_gettime.argtypes = [c_int, POINTER(Timespec)]
libc.clock_gettime.restype = c_int

# Define constants from linux/time.h
CLOCK_REALTIME = 0
CLOCK_MONOTONIC = 1

class HexClock:
    def __init__(self, hex_ptp_clock_env_var):
        if hex_ptp_clock_env_var:
            print(f"Using PTP clock from {hex_ptp_clock_env_var}.")
            self._file = open(hex_ptp_clock_env_var, 'rb')
            self.clock_id = fd_to_clockid(self._file.fileno())
            # 不能 close file, 需要存起来。所以这里还得用 class 把 file 存起来。
        else:
            print("Using monotonic clock.")
            self.clock_id = CLOCK_MONOTONIC

    def now_ms(self):
        ts = Timespec()
        if libc.clock_gettime(self.clock_id, ctypes.byref(ts)) != 0:
            errno = ctypes.get_errno()
            raise OSError(errno, os.strerror(errno))
        secs = ts.tv_sec
        nanos = ts.tv_nsec
        return secs * 1000 + nanos // 1_000_000


def fd_to_clockid(fd):
    # 辅助转换ID的函数
    # Equivalent to the FD_TO_CLOCKID macro
    return ((~fd) << 3) | 3

def main():
    # 从环境变量读取 ptp 时钟路径。如果没有设置环境变量，那就读取本机 monotonic 时钟。
    # 另外，读ptp时钟取会比读CPU时间慢，这个是需要知道的。应该不会有什么性能影响，
    # 如果真的有大的性能影响，其实就每秒同步10次 ptp 时钟与CPU时间的差平滑一下就完了。
    hex_clock = HexClock(os.getenv('HEX_PTP_CLOCK'))
    count = 0
    while True:
        # 一毫米就读一次，测试对CPU的影响大小。但是 100 ms 才 print 一次，消除 print 的性能开销
        count += 1
        time.sleep(0.001)  # Sleep for 1 ms
        try:
            ms = hex_clock.now_ms()
            if count % 100 == 0:
                print(ms)
        except OSError as err:
            print(f"Failed to read PTP clock: {err}")

# Run the main function
main()
