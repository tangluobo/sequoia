"""Check native source-CSS header metrics and single grid seams in owned captures."""
import hashlib
import json
from pathlib import Path
import sys
from PIL import Image

reference,candidate=map(Path,sys.argv[1:])
validation=json.loads((candidate/'validation.json').read_text(encoding='utf-8-sig'))
assert validation['sharedConfigurationUnchanged'] and not validation['savedHostsContacted']
def close(actual,expected):return all(abs(a-b)<=1 for a,b in zip(actual,expected))
def tabs(image,rows,origin,selected,limit=None):
    x,y=origin
    for index,row in enumerate(sorted(rows,key=lambda row:row['x'])[:limit]):
        left=x+round(row['x']);right=left+round(row['width']);top=y+round(row['y']);bottom=top+round(row['height'])
        fill=(252,255,255) if index==selected else (224,224,224)
        for point,expected in [((left+2,top+1),fill),((right-3,top+1),fill),((right-1,top+2),(192,192,192)),((left+2,bottom-1),(0,0,0) if index==selected else (192,192,192))]:
            assert close(image.getpixel(point),expected),(point,image.getpixel(point),expected,row['text'])

proof=[]
for entry in validation['captures']:
    path=Path(entry['path']);assert hashlib.sha256(path.read_bytes()).hexdigest().upper()==entry['sha256']
    image=Image.open(path).convert('RGB');state=entry['state'];details={'state':state}
    if state.startswith('designer-'):
        assert image.size==(896,650)
        multi=state=='designer-multi';index=0 if multi else int(state.rsplit('-',1)[1])
        native=Image.open(reference/(('design-multi-' if multi else 'design-')+str(index)+'.png')).convert('RGB')
        rows=json.loads((reference/'design.png.json').read_text())
        tabs(image,rows,(0,35),index)
        samples=[(80,2),(80,34),(500,640)]
        if index==0:
            for edge in (15,165,285,345,405,465,535,655,855):
                samples.extend([(edge,64),(edge-1,64),(edge+1,64)])
            samples.extend([(0,60),(80,90),(80,91),(80,119),(80,120),(80,121),(870,70)])
            if multi:
                for y in (149,150,151,179,180,181):samples.append((80,y))
        for point in samples:
            assert close(image.getpixel(point),native.getpixel(point)),(state,point,image.getpixel(point),native.getpixel(point))
        details.update(toolbarHeight=35,tabHeight=25,singlePixelGridVerified=index==0)
    elif state.startswith(('kafka','rocketmq')):
        assert image.size==((900,900) if state.endswith('narrow') else (1200,900))
        source='mq-kafka' if state.startswith('kafka') else 'mq'
        rows=json.loads((reference/(source+'.png.json')).read_text())
        tabs(image,[row for row in rows if row['y']==0],(304,90),1)
        # Narrow panes clip later headers into the overflow button; check the
        # fully visible prefix and leave popup routing to the pointer test.
        tabs(image,[row for row in rows if row['y']>0],(304,90),0,2 if state.endswith('narrow') else None)
        details.update(mainTabHeight=33,queryTabHeight=33,narrowOverflow=state.endswith('narrow'))
    else:
        assert image.size==(896,700)
        selected=1 if state=='script_result' else 0
        rows=json.loads((reference/('query-selected.png.json' if selected else 'query.png.json')).read_text())
        if state=='information':rows=rows[:1]
        else:rows=rows[:3]
        current=sorted(rows,key=lambda row:row['x'])[selected]
        left=round(current['x'])+2;right=left+round(current['width'])-5
        bottom=next(y for y in range(200,650) if all(close(image.getpixel((x,y)),(0,0,0)) for x in range(left,right)))
        top=bottom-24
        tabs(image,rows,(0,top),selected)
        details.update(tabHeight=25,tabTop=top,selected=selected)
    proof.append(details)
result={'reference':str(reference),'captures':proof,'comparedOriginalGeometryAndColors':True,'allGlyphPixelsIdentical':False}
(candidate/'style-proof.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
print(json.dumps(result,indent=2))
