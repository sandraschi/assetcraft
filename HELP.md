# assetcraft help

Task-oriented guide. Every command also exists as an MCP tool ([docs/TOOLS.md](docs/TOOLS.md)); add `--json` for machine-readable output; `assetcraft <command> --help` lists all options.

## Registering assets

```powershell
assetcraft add GOL-002 "Mini PC" --category it --in-service 09.11.2026 --cost "689,00" --holder "Joe Mocky" --warranty 09.11.2029
assetcraft add CAR-002 "Lieferwagen" --category car --in-service 01.03.2026 --cost "28.500,00"
assetcraft add MON-003 "Monitor" --category it --in-service 02.11.2026 --cost "349,00"             # GWG: written off at once
assetcraft add MON-004 "Monitor" --category it --in-service 02.11.2026 --cost "349,00" --no-gwg    # normal AfA instead
assetcraft list
assetcraft list --text joe --all
assetcraft show GOL-002
assetcraft update GOL-002 --holder "Anna Mockova" --location "Büro 2"
```

The cost is the AfA base: enter the **net** amount if you can deduct the VAT. Categories set the default useful life and the EKR account (`assetcraft categories`; cars need at least 8 years).

## Depreciation (AfA)

```powershell
assetcraft schedule GOL-002          # full plan
assetcraft afa 2026                  # all assets, totals per account (preview)
assetcraft afa 2026 --emit           # send to buchcraft as drafts
```

Rules: straight-line, calendar year, half a year's AfA when the asset is used six months or less in a year (first and disposal year), GWG at or below the limit (`assetcraft gwg`, default 1.000,00) in full in the first year. Remainder cents go to the final year.

After `--emit`, run `buchcraft inbox-drain` and approve the drafts there. Each asset and year is sent once. From then on cost, in-service date, life and GWG flag of that asset are frozen.

## Disposal

```powershell
assetcraft dispose LAP-001 --date 30.04.2026 --proceeds "150,00" --note "an Anna verkauft"
```

Shows the disposal-year AfA, the book value and the gain or loss. The disposal-year AfA comes through `afa --emit` as usual; book the residual value and the proceeds in buchcraft yourself.

## Licences

```powershell
assetcraft license add "Office suite" --seats 5 --cost "660,00" --renews 01.03.2027 --key-location "Bitwarden: office admin"
assetcraft license assign "Office suite" "Joe Mocky"
assetcraft license unassign "Office suite" "Joe Mocky"
assetcraft license seats "Office suite" 6
assetcraft license audit             # over-allocated, unused, renewals due
```

Never store the licence key itself; assetcraft refuses something that looks like a key.

## Deadlines and expiries

```powershell
assetcraft deadline add "TLS mockfirma.at" --due 28.11.2026 --kind certificate
assetcraft deadline add "Pickerl" --due 05.11.2027 --kind inspection --tag CAR-001
assetcraft expiries --days 60        # warranties, licence renewals, deadlines; overdue first
assetcraft deadline done 3
```

## Maintenance, labels, import

```powershell
assetcraft maintenance CAR-001 service --cost "384,00" --note "Jahresservice"
assetcraft label GOL-002 D:\Temp\gol-002.svg          # 60 x 25 mm, QR code ASSET:GOL-002
assetcraft import D:\Temp\inventory.csv --dry-run      # tag;name;category;in_service;cost[;serial;location;holder]
assetcraft import D:\Temp\inventory.csv
```

The import is all-or-nothing: one bad row and nothing is stored.

## Troubleshooting

- **"AfA basis is frozen"**: AfA for that asset was already sent to buchcraft; correct it there with a Storno and register a corrected asset.
- **"needs at least 8 years"**: cars (EStG section 8(6)).
- **"looks like a licence key"**: store where the key lives, not the key.
- **MCP cannot write the label or read the CSV**: start the server with `--allow-path <folder>`.
