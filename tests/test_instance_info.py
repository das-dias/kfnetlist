"""Instance metadata survives serialization and netlist transformations."""

from types import SimpleNamespace
from typing import cast

import pytest
from kfnetlist import Netlist, NetlistInstance, PlacedInstance, PlacedNetlist
from kfnetlist.extract._algo import _InstanceLike, _create_inst_entry


@pytest.mark.parametrize("cls", [NetlistInstance, PlacedInstance])
def test_instance_info_roundtrip_and_snapshots(cls) -> None:
    source = {"nested": {"values": [1, 2.0, None, True]}, "measure": "spectrum"}
    inst = cls("pdk", "child", {}, None, "ref", info=source)
    expected = inst.info
    source["nested"]["values"].append(3)
    snapshot = inst.info
    snapshot["nested"]["values"].append(4)
    assert inst.info == expected
    assert cls.from_dict(inst.to_dict(), name="ref") == inst
    assert cls.from_json(inst.to_json(), name="ref") == inst
    inst.info = {"measure": "power"}
    assert inst.info == {"measure": "power"}
    assert inst != cls("pdk", "child", {}, None, "ref", info=expected)


@pytest.mark.parametrize("cls", [NetlistInstance, PlacedInstance])
def test_instance_info_default_and_legacy_payload(cls) -> None:
    inst = cls("pdk", "child", info=None)
    assert inst.info == {}
    assert "info" not in inst.to_dict()
    assert cls.from_dict(inst.to_dict()).info == {}
    assert cls.from_json(inst.to_json()).info == {}
    payload = inst.to_dict() | {"info": {}}
    assert cls.from_dict(payload).info == {}


@pytest.mark.parametrize("cls", [NetlistInstance, PlacedInstance])
@pytest.mark.parametrize("invalid", [[], "text", 3, {"bad": object()}])
def test_instance_info_rejects_invalid_values(cls, invalid) -> None:
    with pytest.raises((TypeError, ValueError)):
        cls("pdk", "child", info=invalid)
    inst = cls("pdk", "child", info={"valid": True})
    with pytest.raises((TypeError, ValueError)):
        inst.info = invalid
    assert inst.info == {"valid": True}


@pytest.mark.parametrize("cls", [Netlist, PlacedNetlist])
def test_netlist_info_preservation_and_comparison(cls) -> None:
    nl = cls()
    source = {"nested": [2.0]}
    inst = nl.create_inst("ref", "pdk", "child", {"length": 2.0}, 2, 3, info=source)
    source["nested"].append(3)
    inst.info = {"local": True}
    assert nl.instances["ref"].info == {"nested": [2.0]}
    assert (nl.instances["ref"].array.na, nl.instances["ref"].array.nb) == (2, 3)
    nl.sort()
    normalized = nl.normalize()
    assert normalized.instances["ref"].settings == {"length": 2}
    assert type(normalized.instances["ref"].info["nested"][0]) is float
    assert cls.from_json(nl.to_json()) == nl
    assert cls.from_dict(nl.to_dict()) == nl
    other = cls()
    other.create_inst(
        "ref", "pdk", "child", {"length": 2.0}, 2, 3, info={"other": True}
    )
    assert other != nl
    assert nl.find_net_difference(other) == {"missing": [], "extra": []}


def test_placement_conversion_and_positional_compatibility() -> None:
    nl = Netlist()
    nl.create_inst("ref", "pdk", "child", info={"measure": "spectrum"})
    placed = PlacedNetlist.from_netlist(nl, cells={"ref": "child"})
    assert placed.instances["ref"].info == {"measure": "spectrum"}
    assert placed.instances["ref"].cell == "child"
    inst = PlacedInstance(
        "pdk", "child", {}, None, "ref", "layout_child", None, info={"measure": "power"}
    )
    assert inst.cell == "layout_child"
    created = placed.create_inst(
        "other", "pdk", "child", {}, 1, 1, "layout_child", None, info=inst.info
    )
    assert created.cell == "layout_child"
    assert placed.instances["other"].info == inst.info


@pytest.mark.parametrize("cls", [Netlist, PlacedNetlist])
def test_flatten_preserves_child_info_without_inheriting_parent_info(cls) -> None:
    child = cls()
    child.create_inst("leaf", "pdk", "leaf", info={"owner": "child", "values": [1]})
    top = cls()
    top.create_inst(
        "sub", "pdk", "child", info={"owner": "parent", "parent_only": True}
    )
    top.create_inst("keep", "pdk", "leaf", info={"owner": "survivor"})
    flat = top.flatten({"child": child}, instance_cell_map={"sub": "child"})
    assert flat.instances["sub.leaf"].info == {"owner": "child", "values": [1]}
    assert flat.instances["keep"].info == {"owner": "survivor"}
    assert top.instances["sub"].info["parent_only"] is True
    flat.remove_instances(["sub.leaf"])
    assert flat.instances["keep"].info == {"owner": "survivor"}
    assert child.instances["leaf"].info == {"owner": "child", "values": [1]}


class _ExtractInstance:
    na = nb = 1
    cell = SimpleNamespace(
        has_factory_name=lambda: False,
        name="child",
        is_library_cell=lambda: False,
        kcl=SimpleNamespace(name="pdk"),
        settings=SimpleNamespace(model_dump=dict),
    )

    def __init__(self, name, named=True, metadata=None):
        self.name = name
        self.named = named
        self.metadata = metadata or {}

    def is_named(self):
        return self.named

    @property
    def info(self):
        if not self.named:
            raise ValueError("Unnamed instances cannot have info")
        return SimpleNamespace(model_dump=lambda: self.metadata)


def test_extraction_named_unnamed_and_legacy_instances() -> None:
    nl = Netlist()
    first = _ExtractInstance("first", metadata={"measure": "power"})
    second = _ExtractInstance("", metadata={"measure": "spectrum"})
    unnamed = _ExtractInstance("generated", named=False)
    legacy = SimpleNamespace(
        name="legacy", na=1, nb=1, cell=first.cell, is_named=lambda: True
    )
    for inst in [first, second, unnamed, legacy]:
        _create_inst_entry(nl, cast(_InstanceLike, inst))
    first.metadata["measure"] = "changed"
    assert nl.instances["first"].info == {"measure": "power"}
    assert nl.instances[""].info == {"measure": "spectrum"}
    assert nl.instances["generated"].info == {}
    assert nl.instances["legacy"].info == {}
