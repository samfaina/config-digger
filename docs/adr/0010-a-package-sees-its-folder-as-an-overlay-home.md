# A package sees its folder as an Overlay home

A Claude Desktop MSIX package sees `%APPDATA%\Claude` with its `LocalCache\Roaming\Claude` laid over it path by path: a file the package holds wins, and the classic one is read otherwise (#306). A lookup order picks a whole folder, so config-digger either showed the package's own files as not read or the classic ones as read by the package. So a home can be an Overlay home over another home of its definition, `over = "<home id>"`: each path resolves to the overlay's file if it exists, else to the other home's, and a folder root lists the union of both by name, the overlay's winning. An Entry read through the overlay from the other home's folder belongs to both homes, with an Item and a State in each. We rejected keeping two homes and marking the classic file Unread wherever the package holds a copy, which is false while the classic app reads it, and roots that list candidate paths, which picks among files but cannot merge the listings of a folder root such as `Claude Extensions`.

## Consequences

- The package's lookup order holds its Overlay home, which it picks while the overlay's folder or the other home's exists; the classic app's order holds the classic home alone.
- Disable, Enable and Remove write an Entry where it lives: in the classic folder for one the overlay reads from there, and the preview says the classic home reads it too. config-digger never copies a file into the package's `LocalCache`.
- An Entry parked from either home has the home whose folder holds its file as its origin, so Enable puts it back in that file even once the package holds a copy of its own; it is listed in each home that reads the file.
- An overlay lies over one home that is not itself an overlay, and has the same roots.
- While the classic folder exists beside the package, its Entries the package holds no copy of are listed in both homes, even when no classic Installation is found.
