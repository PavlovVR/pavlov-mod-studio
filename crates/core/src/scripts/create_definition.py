"""Runs inside headless Unreal to create a new mod's definition asset."""
import json
import os

import unreal

plugin = os.environ["PMS_PLUGIN"]
mount = "/" + plugin
result = {}
try:
    unreal.AssetRegistryHelpers.get_asset_registry().scan_paths_synchronous([mount], True)
    definition_class = getattr(unreal, os.environ["PMS_CLASS"])
    factory = unreal.DataAssetFactory()
    factory.set_editor_property("data_asset_class", definition_class)
    tools = unreal.AssetToolsHelpers.get_asset_tools()
    asset = tools.create_asset("definition", mount, definition_class, factory)
    if asset is None:
        raise RuntimeError("Unreal could not create the definition; is the plugin mounted?")
    asset.set_editor_property("UniqueId", plugin)
    if not unreal.EditorAssetLibrary.save_loaded_asset(asset, False):
        raise RuntimeError("could not save the definition")
    result = {"class": asset.get_class().get_name(), "version": asset.get_editor_property("Version")}
except Exception as error:
    result = {"error": str(error)}

with open(os.environ["PMS_OUT"], "w") as out:
    json.dump(result, out)
