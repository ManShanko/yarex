yarex (Yet Another Resource EXtractor)
--------------------------------------
A tool for extracting bundle resources from applications made with [Stingray](https://wikipedia.org/wiki/Autodesk_Stingray).

It supports bundle format v6 used in Vermintide 2 (VT2) and v5 used in VT2 mods.

It does not support the stream format (used for wwise audio).

### Info

yarex caches work to disk. Default file is `yarex.idx` but can be changed with `-c`/`--cache`.

By default yarex tries to find the VT2 bundle directory, but a different directory can be used with `-d`/`--dir`. If loading a cache file than yarex uses the directory stored in it.

### Examples

Extract all files with [known file names](#hash-lookup):
```
yarex -e *
```

Extract all files and fallback to using the file name hash for unknown files:
```
yarex --hash-fallback -e *
```

Extract all texture files and fallback for unknown files:
```
yarex --hash-fallback -e *.texture
```

Extract `scripts/boot` file:
```
yarex -e scripts/boot
yarex -e scripts/boot.lua
yarex -e scripts/boot.*
```

Partial matching for file names is not supported.

### Hash Lookup

yarex supports reverse hash lookup. By default it uses `dictonary.txt`. Change the dictionary loaded with `-k`/`--keys`.

**NOTE:** Some file types (`lua` and `wwise_dep`) always have known names due to how they are stored.

### File Types

`texture` files, at least for VT2, are all DDS files and extract with that extension. To convert DDS to a different image format see [Texconv](https://github.com/microsoft/DirectXTex/wiki/Texconv) from Microsoft. I've had success with converting to BMP as opposed to PNG or JPG. For example `texconv -ft BMP a9c9c2c33ecf18ee.dds`

`lua` files are LuaJIT 2.1 bytecode files. See https://github.com/Aussiemon/ljd for a decompiler.
