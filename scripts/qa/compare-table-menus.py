"""Compare measured original Java menus against actual native egui input captures.
Geometry/palette/selection checks deliberately do not assert pixel-identical font rasterization.
"""
import argparse
import json
from pathlib import Path
from PIL import Image
parser=argparse.ArgumentParser()
parser.add_argument('reference',type=Path)
parser.add_argument('candidate',type=Path)
parser.add_argument('--output',required=True,type=Path)
args=parser.parse_args()
def read(path):return json.loads(path.read_text(encoding='utf-8-sig'))
def darkest(image,node,label_key):
    x,y=int(node['x']),int(node['y'])
    return min(min(pixel) for pixel in image.crop((x,y,x+45,y+12)).get_flattened_data())
results=[]
for case in ['row-menu','multi-row-menu','cell-menu']:
    reference=read(args.reference/(case+'.png.layout.json'))
    candidate=read(args.candidate/(case+'.state.json'))
    menu=read(args.candidate/(case+'.menu.json'))
    assert menu['pointerEventsInjected'] and not menu['osInputUsed']
    assert not reference['networkRequested'] and not candidate['databaseRequested'] and not candidate['systemClipboardWritten']
    left,top,width,height=candidate['contextMenuRect']
    assert abs(width-reference['layoutWidth'])<0.1,(case,width,reference['layoutWidth'])
    assert abs(height-reference['layoutHeight'])<0.1,(case,height,reference['layoutHeight'])
    labels=[item['text'] for item in reference['items'] if not item['separator']]
    nodes=[node for node in menu['positions'] if top<=node['y']<=top+height and left<=node['x']<=left+width]
    assert [node['label'] for node in sorted(nodes,key=lambda node:node['y'])]==labels,(case,nodes)
    native={node['label']:node for node in nodes}
    original={node['text']:node for node in reference['nodes'] if node.get('text') in labels}
    shadow_x=(reference['width']-reference['layoutWidth'])/2
    shadow_y=(reference['height']-reference['layoutHeight'])/2-8
    delta=[]
    for label in labels:
        expected,actual=original[label],native[label]
        dx=actual['x']-left-(expected['x']-shadow_x)
        dy=actual['y']-top-(expected['y']-shadow_y)
        assert abs(dx)<=0.75 and abs(dy)<=0.75,(case,label,dx,dy)
        assert actual['height']<=20,(case,label,actual['height'])
        delta.append({'label':label,'dx':dx,'dy':dy})
    expected_cells=sorted((cell['row'],cell['column']-1)for cell in reference['selectedCells']if cell['column']>0)
    assert expected_cells==[tuple(cell)for cell in candidate['selectedCells']],(case,expected_cells,candidate['selectedCells'])
    assert reference['selectedRows']==candidate['selectedRows']
    assert candidate['rowMode']==(case!='cell-menu')
    ref_image=Image.open(args.reference/(case+'.png')).convert('RGBA')
    white=Image.new('RGBA',ref_image.size,'white');white.alpha_composite(ref_image);ref_image=white.convert('RGB')
    new_image=Image.open(args.candidate/(case+'.png')).convert('RGB')
    assert new_image.size==(896,650)
    expected_gray=darkest(ref_image,original['删除 记录'],'text')
    actual_gray=darkest(new_image,native['删除 记录'],'label')
    assert abs(actual_gray-expected_gray)<=2,(case,expected_gray,actual_gray)
    results.append({'case':case,'menuBody':[width,height],'labels':labels,'relativeTextDeltas':delta,'selectedRows':candidate['selectedRows'],'selectedCells':candidate['selectedCells'],'disabledInkReference':expected_gray,'disabledInkCandidate':actual_gray})
result={'passed':True,'nativePointerInput':True,'originalJavaSelectionMatched':True,'cases':results,'fullPixelParity':False}
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(json.dumps({'passed':True,'cases':len(results),'body':[114,255],'rasterPixelParity':False}))
