"""Load a plugin into this running editor, the way Unreal's New Plugin window does.

FPluginUtils::LoadPlugin isn't exposed to Python, so this calls the function the engine exports.
The structures below match Unreal 5.1, so anything else is refused rather than guessed at.
DESCRIPTOR is set by the app above this line.
"""
import ctypes
from ctypes import POINTER, Structure, byref, c_bool, c_char_p, c_int32, c_void_p, c_wchar_p

import unreal

SYMBOL = b"?LoadPlugin@FPluginUtils@@SA?AV?$TSharedPtr@VIPlugin@@$00@@AEBVFString@@AEAUFLoadPluginParams@1@@Z"


class FString(Structure):
    _fields_ = [("data", c_void_p), ("num", c_int32), ("max", c_int32)]


class FLoadPluginParams(Structure):
    _fields_ = [
        ("scan_assets", c_bool),
        ("select_in_browser", c_bool),
        ("enable_in_project", c_bool),
        ("update_search_path", c_bool),
        ("already_loaded", c_bool),
        ("fail_reason", c_void_p),
    ]


class SharedPtr(Structure):
    _fields_ = [("plugin", c_void_p), ("references", c_void_p)]


if not unreal.SystemLibrary.get_engine_version().startswith("5.1."):
    raise RuntimeError("this only works with Unreal 5.1")

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.GetModuleHandleW.restype = c_void_p
kernel.GetModuleHandleW.argtypes = [c_wchar_p]
kernel.GetProcAddress.restype = c_void_p
kernel.GetProcAddress.argtypes = [c_void_p, c_char_p]
module = kernel.GetModuleHandleW("UnrealEditor-PluginUtils.dll")
address = kernel.GetProcAddress(module, SYMBOL) if module else None
if not address:
    raise RuntimeError("this editor can't load plugins while it runs")

# A function that returns a class writes it through a hidden first pointer.
load_plugin = ctypes.CFUNCTYPE(c_void_p, POINTER(SharedPtr), POINTER(FString), POINTER(FLoadPluginParams))(address)
text = ctypes.create_unicode_buffer(DESCRIPTOR)
path = FString(ctypes.cast(text, c_void_p), len(DESCRIPTOR) + 1, len(DESCRIPTOR) + 1)
params = FLoadPluginParams(True, True, False, False, False, None)
result = SharedPtr()
load_plugin(byref(result), byref(path), byref(params))
if not result.plugin:
    raise RuntimeError("Unreal wouldn't load the plugin")
print("already" if params.already_loaded else "loaded")
