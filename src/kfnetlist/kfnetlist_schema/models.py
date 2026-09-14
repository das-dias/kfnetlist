"""Rust-backed circuit and PIC document types. Fields return owned snapshots."""

from kfnetlist._native import (
    TerminalDirection as TerminalDirection,
    SignalDomain as SignalDomain,
    SIPrefix as SIPrefix,
    PrefixedValue as PrefixedValue,
    ParameterValue as ParameterValue,
    Parameter as Parameter,
    ModelInterface as ModelInterface,
    ModelReference as ModelReference,
    Terminal as Terminal,
    TerminalReference as TerminalReference,
    Connection as Connection,
    Bus as Bus,
    ExternalModule as ExternalModule,
    ProtoModuleReference as ProtoModuleReference,
    ProtoModule as ProtoModule,
    ProtoCircuit as ProtoCircuit,
    ArraySpec as ArraySpec,
    Instance as Instance,
    Module as Module,
    TopLevelModule as TopLevelModule,
    Net as Net,
    Netlist as Netlist,
    NetlistArray as NetlistArray,
    NetlistInstance as NetlistInstance,
    NetlistPort as NetlistPort,
    PortRef as PortRef,
    PortArrayRef as PortArrayRef,
)

ModuleNetlist = Netlist
InstanceRef = NetlistInstance
