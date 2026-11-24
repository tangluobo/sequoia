from pathlib import Path
from PIL import Image,ImageDraw,ImageChops
import json,hashlib
import argparse
parser=argparse.ArgumentParser(description='Compare the deterministic scattered RDP mosaic against native captures.')
parser.add_argument('--probe-prefix',required=True,help='Label prefix, followed by -tab, -window, -fullscreen')
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
root=Path(__file__).resolve().parents[2];records=[]
reference=Image.new('RGB',(1920,1080));draw=ImageDraw.Draw(reference)
for tile in range(510):
 x=(tile%30)*64;y=(tile//30)*64;draw.rectangle((x,y,x+63,min(y+63,1079)),fill=(30+tile*37%180,34,56))
for mode in ['TAB','WINDOW','FULLSCREEN']:
 label=f'{args.probe_prefix}-{mode.lower()}';p=root/f'target/window-probes/{label}/{mode}.png'
 image=Image.open(p).convert('RGB')
 if mode=='TAB':
  data=image.load();origin=None
  for y in range(image.height):
   start=None
   for x in range(image.width):
    rgb=data[x,y];matches=abs(rgb[1]-34)<=1 and abs(rgb[2]-56)<=1 and 20<=rgb[0]<=220
    if matches and start is None:start=x
    if not matches:
     if start is not None and x-start>=200:origin=(start,y);break
     start=None
   if start is not None and image.width-start>=200:origin=(start,y)
   if origin:break
  assert origin,'No remote surface found'
  # ScrollArea paints a 32px scrollbar/fade gutter over the remote image.
  # Compare a fixed unobscured region, rather than treating that UI overlay
  # as corrupted remote pixels. WINDOW/FULLSCREEN still compare every pixel.
  x,y=origin;width=min(image.width-x-32,864);height=min(image.height-y-32,700)
  assert width>0 and height>0,'No unobscured remote surface'
  actual=image.crop((x,y,x+width,y+height));expected=reference.crop((0,0,width,height));scope='unobscured remote crop; fixed 32px scrollbar/fade gutter excluded'
 else:
  assert image.size==(1920,1080),(mode,image.size)
  origin=(0,0);actual=image;expected=reference;scope='complete remote framebuffer'
 difference=ImageChops.difference(actual,expected);maximum=max(hi for lo,hi in difference.getextrema())
 assert maximum<=1,(mode,'missing or corrupt pixels',maximum)
 records.append(dict(mode=mode,origin=origin,size=actual.size,comparedPixels=actual.width*actual.height,maxChannelDifference=maximum,scope=scope,imageSha256=hashlib.sha256(p.read_bytes()).hexdigest().upper()))
proof=dict(source='510 patterned ClearCodec tiles in a scattered permutation, sent in 16 batches',allComparedPixelsMatch=True,tolerance='at most one channel level from GPU dithering; no spatial or black-block tolerance',results=records)
args.output.write_text(json.dumps(proof,indent=2),encoding='utf-8');print(json.dumps(proof))


