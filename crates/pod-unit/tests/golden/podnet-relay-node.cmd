:: Written by podnet unit. No secret is in these commands.
sc.exe create podnet-relay-node binPath= "\"C:\Program Files\PodUtils\podnet.exe\" relay node --state \"C:\ProgramData\podnet\relay node\"" start= auto obj= "NT SERVICE\podnet-relay-node" DisplayName= "podnet relay node"
sc.exe description podnet-relay-node "podnet relay node. Written by podnet unit."
sc.exe sidtype podnet-relay-node restricted
sc.exe privs podnet-relay-node SeChangeNotifyPrivilege
sc.exe failure podnet-relay-node reset= 86400 actions= restart/5000/restart/5000/restart/5000
sc.exe failureflag podnet-relay-node 1
reg.exe add HKLM\SYSTEM\CurrentControlSet\Services\podnet-relay-node /v Environment /t REG_MULTI_SZ /d RUST_LOG=info\0TUNNEL_TOKEN_FILE=C:\ProgramData\podnet\tunnel.token /f
