schema = 3

[moe.getaurora.daturaxoxo.2000/everlight/settings]
folder = "AuroraMods"
debug  = true
method = "sigbypass"

[[moe.getaurora.daturaxoxo.2000/everlight/target]]
label   = "NTE / UE 5.4+ generic (gacha UE5)"
target  = "HTGame.exe"
method  = "sigbypass"
type    = 0
pattern = "48 8D ?? ?? ?? ?? ?? E9 ?? ?? ?? ?? CC CC CC CC 48 83 EC 28 33 D2 48 8D 4C 24 30 E8 ?? ?? ?? ?? 48 8B C8 E8 ?? ?? ?? ?? 48 89 ?? ?? ?? ?? ?? 48 83 C4 28 C3 CC CC CC CC CC CC CC CC CC CC CC CC 48 8D ?? ?? ?? ?? ?? E9"
offset  = 0x47
bytes   = [0xB0, 0x01, 0xC3]