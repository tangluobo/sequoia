"""Compare owned JavaFX and Rust headers; geometry/colors, not rasterizer identity."""
import argparse,json
from pathlib import Path
from PIL import Image,ImageChops

parser=argparse.ArgumentParser()
parser.add_argument('reference',type=Path)
parser.add_argument('candidate',type=Path)
parser.add_argument('--header-divider',action='store_true')
args=parser.parse_args()
def near(pixel,color):return all(abs(a-b)<=1 for a,b in zip(pixel,color))
def ink(image,rect,lcd=False):
    crop=image.crop(rect);mask=Image.new('L',crop.size)
    # JavaFX popup text uses LCD subpixels. Compare luminance there; requiring
    # every RGB channel to be dark incorrectly discards colored descenders.
    mask.putdata([255 if (sum(a*b for a,b in zip(pixel,(.2126,.7152,.0722))) if lcd else max(pixel))<160 else 0 for pixel in crop.get_flattened_data()])
    box=mask.getbbox()
    return [box[0]+rect[0],box[1]+rect[1],box[2]+rect[0],box[3]+rect[1]] if box else None
proof=[]
nodes=json.loads((args.reference/'plain.png.json').read_text())['nodes']
tabs=[node for node in nodes if node['style']=='tab']
expected=[int(tab['width']) for tab in tabs]
for state in ['plain','selected']:
    original=Image.open(args.reference/(state+'.png')).convert('RGB')
    capture=Image.open(args.candidate/(state+'.png')).convert('RGB')
    assert capture.size==(1200,900),capture.size
    image=capture.crop((304,53,1199,153))
    borders=[x for x in range(image.width) if sum(near(image.getpixel((x,y)),(192,192,192)) for y in range(37))>=34]
    assert len(borders)==4,borders
    actual=[borders[0]+1]+[b-a for a,b in zip(borders,borders[1:])]
    assert actual[:3]==expected[:3],(actual,expected)
    assert abs(actual[3]-expected[3])<=1,(actual,expected)
    offset=0;labels=[]
    for index,(width,reference_tab) in enumerate(zip(actual,tabs)):
        selected=index==(1 if state=='selected' else 0)
        assert near(image.getpixel((offset+4,3)),(252,255,255) if selected else (224,224,224))
        assert near(image.getpixel((offset+4,36)),(0,0,0) if selected else (192,192,192))
        assert near(image.getpixel((offset+4,37)),(255,255,255)), 'header leaked into content'
        label=next(node for node in nodes if node['style']=='tab-label' and reference_tab['x']<=node['x']<reference_tab['x']+reference_tab['width'])
        # Exclude the graphic from the label's ink measurement.
        x=int(label['x'])+(22 if index in [1,2] else 0)
        reference_box=ink(original,(x,5,int(label['x']+label['width']),31))
        candidate_box=ink(image,(offset+12+(22 if index in [1,2] else 0),5,offset+width-13-(16 if index!=2 else 0),31))
        assert reference_box and candidate_box,(index,reference_box,candidate_box)
        assert all(abs(a-b)<=1 for a,b in zip(reference_box,candidate_box)),(index,reference_box,candidate_box)
        labels.append({'title':label['text'],'originalInk':reference_box,'candidateInk':candidate_box})
        offset+=width
    assert near(image.getpixel((800,3)),(236,236,234))
    image.save(args.candidate/(state+'-header.png'))
    proof.append({'state':state,'height':37,'originalWidths':expected,'candidateWidths':actual,'labels':labels,'backgroundsAndBottomBordersMatch':True})
original=Image.open(args.reference/'overflow.png').convert('RGB')
capture=Image.open(args.candidate/'overflow.png').convert('RGB')
assert capture.size==(800,600),capture.size
image=capture.crop((304,53,799,153));image.save(args.candidate/'overflow-header.png')
arrow_original=ink(original,(479,13,495,32));arrow_candidate=ink(image,(479,13,495,32))
assert arrow_original and arrow_candidate and all(abs(a-b)<=1 for a,b in zip(arrow_original,arrow_candidate)),(arrow_original,arrow_candidate)
assert near(image.getpixel((473,3)),(236,236,234)), 'missing original 10 px overflow gap'
result={'captures':proof,'overflow':{'width':16,'gap':10,'originalArrowInk':arrow_original,'candidateArrowInk':arrow_candidate},'originalHoverUnchanged':ImageChops.difference(Image.open(args.reference/'plain.png'),Image.open(args.reference/'hover.png')).getbbox() is None,'allPixelsIdentical':False,'scope':'Same owned inert header cases; colors and geometry, ink bounds within 1px. Last long title differs by at most 1px; gradient/antialiasing rasterizers are not asserted identical.'}
metadata=json.loads((args.candidate/'popup.popup.json').read_text(encoding='utf-8-sig'))
rows=metadata['rows']
native=json.loads((args.reference/'popup.png.popup.json').read_text())
native_rows=[node for node in native if node['style'].startswith('menu-item')]
assert len(rows)==len(native_rows)==4
assert [row['title'] for row in rows]==[label['text'] for label in native if label['style']=='label']
assert rows[1]['label']=='sampledb.items', 'JavaFX overflow mnemonic parsing differs'
assert [row['selected'] for row in rows]==[True,False,False,False]
for actual,expected_row in zip(rows,native_rows):
    assert actual['height']==expected_row['height']==23
    assert abs(actual['width']-expected_row['width'])<=1
first=rows[0];native_first=native_rows[0]
left,top=int(first['x']-1),int(first['y']-4)
popup=Image.open(args.candidate/'popup.png').convert('RGB').crop((left,top,left+284,top+100))
native_left,native_top=int(native_first['x']-1),int(native_first['y']-4)
native_popup=Image.open(args.reference/'popup.png.popup.png').convert('RGB').crop((native_left,native_top,native_left+284,native_top+100))
popup.save(args.candidate/'popup-menu.png')
for index in range(4):
    bounds=(21,4+index*23,272,27+index*23)
    original_ink,candidate_ink=ink(native_popup,bounds,True),ink(popup,bounds,True)
    assert original_ink and candidate_ink and all(abs(a-b)<=1 for a,b in zip(original_ink,candidate_ink)),(index,original_ink,candidate_ink)
result['popup']={'height':100,'width':284,'rowHeight':23,'checkmarkSelectedRow':0,'iconsShown':False,'textInkBoundsWithin1px':True,'configurationTitlesUnchanged':True}
if args.header_divider:
    for state in ('plain','selected','overflow','popup'):
        image=Image.open(args.candidate/(state+'.png')).convert('RGB')
        for left,right in ((63,303),(304,image.width-1)):
            assert all(near(image.getpixel((x,52)),(217,217,215)) for x in range(left,right)),state
    result['titleDivider']={'fullWidth':True,'y':52,'states':['plain','selected','overflow','popup']}
(args.candidate/'geometry.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=False))
