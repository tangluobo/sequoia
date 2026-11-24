from pathlib import Path
from PIL import Image
import argparse,json
parser=argparse.ArgumentParser();parser.add_argument('--full',required=True);parser.add_argument('--red',required=True);args=parser.parse_args()
root=Path(__file__).resolve().parents[2]
base=root/'target/window-probes'
def folder(label):
 if not label or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in label): raise ValueError('Expected an owned probe label')
 return base/label
full_dir,red_dir=folder(args.full),folder(args.red)
for p in [full_dir,red_dir]:
 metrics=json.loads((p/'adb.video.json').read_text(encoding='utf-8-sig'))
 assert metrics['pixelFormat']=='GPU-I420',metrics
full=Image.open(full_dir/'adb.png').convert('RGB');red=Image.open(red_dir/'adb.png').convert('RGB')
points=[(x,y) for x in [350,700,1100] for y in [300,500,700]]
def samples(image): return [image.getpixel((int(x*image.width/1200),int(y*image.height/900))) for x,y in points]
colors,reds=samples(full),samples(red)
def reference(n):
 y=(32+n*11%180)-16;u=(16+n*7%220)-128;v=(16+n*17%220)-128
 base=y*255/219
 return tuple(max(0,min(255,int(value))) for value in [base+v*255/224*1.402,base-v*255/224*1.402*.299/.587-u*255/224*1.772*.114/.587,base+u*255/224*1.772])
error=min(max(abs(a-b) for a,b in zip(colors[0],reference(n))) for n in range(90))
spread=max(max(abs(a-b) for a,b in zip(color,colors[0])) for color in colors)
red_error=max(max(abs(a-b) for a,b in zip(color,(254,0,0))) for color in reds)
assert error<=6,(colors,error)
assert spread<=2,(colors,spread)
assert red_error<=2,(reds,red_error)
result={'fullFrameColor':colors[0],'fullFrameBt601MaxChannelError':error,'fullFrameUniformMaxChannelSpread':spread,'javaRedMaxChannelError':red_error,'javaRedSampleColors':reds,'pixelCoordinatesAt1200x900':points,'actualGpuShaderRendered':True,'physicalScanoutMeasured':False}
(full_dir/'color-validation.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print(json.dumps(result))
