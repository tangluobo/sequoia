package com.tangluobo.tomato.module.connect.view;
import java.nio.file.*;
import com.google.gson.*;
/** Executes the unchanged original duplicate calculation, including UTF-16 positions. */
public class SqlDuplicateGolden {
 public static void main(String[] args)throws Exception {
  String[] texts={"","abc","abc\ndef","abc\r\ndef","abc\r\ndef\nlast","\n","a\n\nb","a\r\n","中文🍅\n下一行","last\r","\r\n\r\n"};
  JsonArray cases=new JsonArray();
  for(String text:texts) {
   for(int caret:new int[]{0,text.length()/2,text.length()}) {
    if(caret>0&&caret<text.length()&&Character.isLowSurrogate(text.charAt(caret)))continue;
    var edit=SqlEditorDuplicateSupport.duplicate(text,caret,caret,caret);add(cases,text,caret,caret,caret,edit);
   }
   if(text.length()>1) {
    int end=text.length();var edit=SqlEditorDuplicateSupport.duplicate(text,0,end,0);add(cases,text,0,end,0,edit);
    edit=SqlEditorDuplicateSupport.duplicate(text,end,0,end);add(cases,text,end,0,end,edit);
   }
  }
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(cases));
 }
 static void add(JsonArray out,String text,int start,int end,int caret,SqlEditorDuplicateSupport.DuplicateEdit edit) {
  JsonObject row=new JsonObject();row.addProperty("text",text);row.addProperty("start",start);row.addProperty("end",end);row.addProperty("caret",caret);row.addProperty("result",text.substring(0,edit.insertionIndex())+edit.text()+text.substring(edit.insertionIndex()));row.addProperty("anchor",edit.selectionAnchor());row.addProperty("cursor",edit.selectionCaret());out.add(row);
 }
}
