# OpenJFX confirmation graphic

`dialog-confirm.png` and `dialog-confirm@2x.png` are unchanged Modena resources
from `org.openjfx:javafx-controls:27-ea+24:win`, the exact JavaFX artifact used by
the original application. They are used to match its standard query dialogs.

Upstream source directory:
https://github.com/openjdk/jfx/tree/master/modules/javafx.controls/src/main/resources/com/sun/javafx/scene/control/skin/modena

OpenJFX is distributed under GPL version 2 with the Classpath exception; see the
included `LICENSE` and `ADDITIONAL_LICENSE_INFO`. `ASSEMBLY_EXCEPTION` is retained
from upstream as well. The PNGs are provided in their original editable form.
No JavaFX runtime is embedded or invoked by the Rust application.

`modena-check.svg` preserves the `.check-box > .box > .mark` path from that
artifact's `modena.css`, with its gray mark and white highlight. It is used by
the original-style connection import/export trees; `backup-check.svg` retains
the distinct custom backup-tree shape.

`dialog-error.png` and `dialog-information.png` are unchanged 48×48 Modena
resources from the same artifact, used by native Rust backup/restore alerts.
Their SHA-256 hashes are respectively
`C70D63B0DBDD737CF0C4840201AE71CDF8F6815BE968B70F53E8301C0FBBCEA0`
and `ADB20F64CD283F9DAAD521F27B6277F9B2CE71CD0C0C937E10B8534332198504`.

`dialog-warning.png` is the unchanged 48×48 Modena warning resource from the same
OpenJFX artifact. SHA-256:
`1286713C8F4ED5799278A1393DDD811E6A160A09BEAD01BEFBCE4362B11D48CF`.
The original RestoreDialog confirmation is measured by
`crates/core/tests/java/RestoreConfirmationOracle.java`; its No button is fired
before any database task is created.

`backup-check.svg` separately reproduces the original Tomato
`connect-tree.css` backup-object-tree check path, under LICENSE-TOMATO-ASSETS.

SHA-256 of the 48×48 resource:
`55D91B7796F84F87D6283C4662992CF476A1B72DA8854349E5B7BDAF816B4277`.
