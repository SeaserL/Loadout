# SPDX-License-Identifier: Apache-2.0
"""Windows-user-protected bridge key. read is exclusively for a private child pipe."""
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import secrets
import subprocess
import sys

ROOT = Path(os.environ["LOCALAPPDATA"]) / "LoadoutWorkBuddyBridge"


class Blob(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("data", ctypes.POINTER(ctypes.c_ubyte))]


def protect(value, decrypt=False):
    if os.name != "nt":
        raise RuntimeError("Windows required")
    buffer = ctypes.create_string_buffer(value)
    source = Blob(len(value), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_ubyte)))
    result = Blob()
    dll = ctypes.WinDLL("crypt32", use_last_error=True)
    fn = dll.CryptUnprotectData if decrypt else dll.CryptProtectData
    fn.argtypes = [ctypes.POINTER(Blob), ctypes.c_void_p, ctypes.c_void_p,
                   ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(Blob)]
    fn.restype = wintypes.BOOL
    if not fn(ctypes.byref(source), None, None, None, None, 1, ctypes.byref(result)):
        raise RuntimeError("DPAPI failed")
    try:
        return ctypes.string_at(result.data, result.size)
    finally:
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.LocalFree.argtypes = [ctypes.c_void_p]
        kernel.LocalFree.restype = ctypes.c_void_p
        kernel.LocalFree(result.data)


def main():
    action = sys.argv[1]
    file = ROOT / "bridge-key.dpapi"
    if action == "init":
        ROOT.mkdir(parents=True, exist_ok=True)
        if not file.exists():
            with file.open("xb") as stream:
                stream.write(protect(secrets.token_urlsafe(32).encode()))
        print("Bridge key ready (Windows DPAPI)")
    elif action == "self-test":
        value = b"synthetic-key-test"
        ciphertext = protect(value)
        assert value not in ciphertext and protect(ciphertext, True) == value
        print("DPAPI: PASS")
    elif action in ("read", "configure-account"):
        key = protect(file.read_bytes(), True).decode()
        if action == "read":
            sys.stdout.write(key)
        else:
            result = subprocess.run([sys.argv[2], "account", "add", "workbuddy", "main", "--api-key-stdin"],
                                    input=key + "\n", text=True, capture_output=True, timeout=60)
            print("Account configuration exit code:", result.returncode)
            print(result.stdout.replace(key, "[REDACTED]"))
            print(result.stderr.replace(key, "[REDACTED]"))
            raise SystemExit(result.returncode)
    else:
        raise RuntimeError("Unknown action")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Bridge key operation failed", file=sys.stderr)
        raise SystemExit(1)
