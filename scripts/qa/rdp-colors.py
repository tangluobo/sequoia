"""Compare the owned static 320x256 RDP fixture in standard TAB/WINDOW captures."""
import argparse
import json
from collections import Counter
from pathlib import Path
from PIL import Image, ImageChops

parser=argparse.ArgumentParser()
parser.add_argument("baseline",type=Path)
parser.add_argument("candidate",type=Path)
parser.add_argument("--candidate-tab-y",type=int,default=89)
args=parser.parse_args()
results=[]
for mode in ("TAB","WINDOW"):
    old=Image.open(args.baseline/(mode+".png")).convert("RGB")
    new=Image.open(args.candidate/(mode+".png")).convert("RGB")
    assert old.size==new.size,(mode,old.size,new.size)
    x,y=(304,89) if mode=="TAB" else (0,0)
    crop=(x,y,x+320,y+256)
    new_y=args.candidate_tab_y if mode=="TAB" else y
    candidate_crop=(x,new_y,x+320,new_y+256)
    difference=ImageChops.difference(old.crop(crop),new.crop(candidate_crop))
    identical=difference.getbbox() is None
    channel_difference=max(high for low,high in difference.getextrema())
    tolerance=1 if mode=="TAB" and new_y!=y else 0
    assert channel_difference<=tolerance,(mode,"remote framebuffer changed",channel_difference)
    matching=Counter(new.crop(candidate_crop).get_flattened_data())[(12,34,56)]
    assert matching>=30000,(mode,matching)
    results.append(dict(mode=mode,crop=list(crop),candidateCrop=list(candidate_crop),framebufferCropPixelsIdentical=identical,framebufferColorsAndGeometryMatch=True,maximumChannelDifference=channel_difference,channelTolerance=tolerance,matchingRemoteColorPixels=matching,captureSize=list(new.size),fullClientPixelsIdentical=ImageChops.difference(old,new).getbbox() is None))
proof=dict(baseline=str(args.baseline),candidate=str(args.candidate),captures=results,scope="Owned remote framebuffer geometry/colors. Translated TAB crops allow 1 channel level; unchanged WINDOW remains exact. Not complete application visual parity.")
(args.candidate/"color-parity.json").write_text(json.dumps(proof,indent=2),encoding="utf-8")
print(json.dumps(proof))
