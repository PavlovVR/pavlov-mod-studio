"""Runs inside headless Unreal to dump its dependency tree for every mod. Reads only."""
import json
import os

import unreal

result = {"packages": {}}
try:
    registry = unreal.AssetRegistryHelpers.get_asset_registry()
    registry.search_all_assets(True)
    options = unreal.AssetRegistryDependencyOptions(
        include_soft_package_references=True,
        include_hard_package_references=True,
        include_searchable_names=True,
        include_soft_management_references=True,
        include_hard_management_references=True,
    )
    plugins = [
        name
        for name in os.listdir(os.path.join(unreal.Paths.project_dir(), "Plugins"))
        if name.startswith("UGC")
    ]
    for plugin in plugins:
        for asset in registry.get_assets_by_path("/" + plugin, recursive=True):
            name = str(asset.package_name)
            if name not in result["packages"]:
                targets = registry.get_dependencies(asset.package_name, options) or []
                result["packages"][name] = sorted(
                    {str(target) for target in targets if str(target).startswith("/UGC")}
                )
except Exception as error:
    result = {"error": str(error)}

with open(os.environ["PMS_OUT"], "w", encoding="utf-8") as out:
    json.dump(result, out, ensure_ascii=False)
