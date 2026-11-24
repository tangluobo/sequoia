"""Verify that the SQL-preview change does not reflow or repaint the table body."""
import argparse, json, pathlib
from PIL import Image, ImageChops
p=argparse.ArgumentParser();p.add_argument('--before',type=pathlib.Path,required=True);p.add_argument('--after',type=pathlib.Path,required=True);p.add_argument('--allow-identical',action='store_true');a=p.parse_args()
load=lambda path:json.loads(path.read_text(encoding='utf-8-sig'))
old=load(a.before/'validation.json');new=load(a.after/'validation.json');checks=[]
for state in ['plain','selected']:
 before=Image.open(a.before/(state+'.png')).convert('RGB');after=Image.open(a.after/(state+'.png')).convert('RGB')
 assert before.size==after.size==(896,650)
 unchanged=ImageChops.difference(before.crop((0,0,896,629)),after.crop((0,0,896,629))).getbbox() is None
 assert unchanged,(state,'table body or toolbar changed')
 bounds=ImageChops.difference(before,after).getbbox();assert (bounds is None and a.allow_identical) or (bounds is not None and bounds[1]>=629),(state,bounds)
 checks.append({'state':state,'toolbarAndGridPixelIdentical':True,'changedBounds':list(bounds) if bounds else None,'clientSize':[896,650],'footerHeight':21})
result={'status':'passed','beforeGuiSha256':old['guiSha256'],'guiSha256':new['guiSha256'],'checks':checks}
(a.after/'layout-validation.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
