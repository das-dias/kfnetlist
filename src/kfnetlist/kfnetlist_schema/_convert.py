"""Compatibility functions delegating schema operations to the Rust core."""

from kfnetlist._native import load_pic_yaml as load_pic_yaml
from .models import Module, Netlist, ProtoCircuit, TopLevelModule


def top_level_module_to_netlists(doc: TopLevelModule) -> dict[str, Netlist]:
    return doc.to_netlists()


def module_to_netlist(mod: Module) -> Netlist:
    return mod.to_netlist()


def netlists_to_top_level_module(
    netlists: dict[str, Netlist], toplevel: str | None = None
) -> TopLevelModule:
    return TopLevelModule.from_netlists(netlists, toplevel=toplevel)


def netlist_to_module(name: str, nl: Netlist) -> Module:
    return Module.from_netlist(name, nl)


def top_level_module_to_proto_circuit(doc: TopLevelModule) -> ProtoCircuit:
    return doc.to_proto_circuit()


def proto_circuit_to_top_level_module(circuit: ProtoCircuit) -> TopLevelModule:
    return TopLevelModule.from_proto_circuit(circuit)
