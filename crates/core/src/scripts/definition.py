"""Runs inside headless Unreal to read or set the Version on a mod definition."""
import json
import os

import unreal

asset_path = os.environ["PMS_ASSET"]
mount = "/" + asset_path.strip("/").split("/")[0]
result = {}
try:
    unreal.AssetRegistryHelpers.get_asset_registry().scan_paths_synchronous([mount], True)
    library = unreal.EditorAssetLibrary
    asset = library.load_asset(asset_path)
    if asset is None:
        raise RuntimeError("definition not found: " + asset_path)
    if "PMS_VERSION" in os.environ:
        asset.set_editor_property("Version", int(os.environ["PMS_VERSION"]))
        if not library.save_loaded_asset(asset, False):
            raise RuntimeError("could not save " + asset_path)
    result = {"class": asset.get_class().get_name(), "version": asset.get_editor_property("Version")}
except Exception as error:
    result = {"error": str(error)}

with open(os.environ["PMS_OUT"], "w") as out:
    json.dump(result, out)
