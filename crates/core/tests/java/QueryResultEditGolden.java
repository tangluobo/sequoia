package com.tangluobo.tomato.module.connect.view;
import com.tangluobo.tomato.module.connect.ConnectType;
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
/** Invokes the unchanged original mapping record, without database or UI. */
public class QueryResultEditGolden {
 static List<Map<String,String>> columns(String[] names,String... keys){var result=new ArrayList<Map<String,String>>();for(var name:names)result.add(Map.of("字段名",name,"主键",Arrays.asList(keys).contains(name)?"是":"否"));return result;}
 static void add(JsonArray out,String name,ConnectType type,String target,List<String> labels,List<String> sources,List<String> tables,List<Map<String,String>> columns){var gson=new GsonBuilder().serializeNulls().create();var item=new JsonObject();item.addProperty("name",name);item.addProperty("kind",type==null?null:type.name());item.addProperty("target",target);item.add("labels",gson.toJsonTree(labels));item.add("sources",gson.toJsonTree(sources));item.add("tables",gson.toJsonTree(tables));item.add("columns",gson.toJsonTree(columns));var plan=QueryResultEditPlan.create(type,target,labels,sources,tables,columns);item.add("plan",gson.toJsonTree(plan));item.addProperty("editable",plan.isEditable());item.addProperty("delete",plan.canDelete());out.add(item);}
 public static void main(String[] args)throws Exception {
  var out=new JsonArray();var labels=List.of("id","name");var source=List.of("id","name");var tables=List.of("items","items");var actual=columns(new String[]{"id","name"},"id");
  for(var type:new ConnectType[]{ConnectType.MYSQL,ConnectType.POSTGRESQL,ConnectType.ORACLE,ConnectType.SSH,null}){
   var prefix=String.valueOf(type)+"/";
   add(out,prefix+"physical",type,"items",labels,source,tables,actual);
   add(out,prefix+"aliases",type,"ITEMS",List.of("alias_id","alias_name"),source,tables,actual);
   add(out,prefix+"expression-before-id",type,"items",List.of("id","real_id","name"),List.of("id","id","name"),List.of("","items","items"),actual);
   add(out,prefix+"expression-only-key",type,"items",labels,source,List.of("","items"),actual);
   add(out,prefix+"duplicate-base-column",type,"items",List.of("id","name","second_id"),List.of("id","name","id"),List.of("items","items","items"),actual);
   add(out,prefix+"partial-composite",type,"items",labels,source,tables,columns(new String[]{"id","name","tenant"},"id","tenant"));
   add(out,prefix+"whole-composite",type,"items",List.of("tenant","id","name"),List.of("tenant","id","name"),List.of("items","items","items"),columns(new String[]{"id","name","tenant"},"id","tenant"));
   add(out,prefix+"no-key-all",type,"items",labels,source,tables,columns(new String[]{"id","name"}));
   add(out,prefix+"no-key-partial",type,"items",List.of("name"),List.of("name"),List.of("items"),columns(new String[]{"id","name"}));
   add(out,prefix+"no-key-untrusted",type,"items",labels,null,null,columns(new String[]{"id","name"}));
   add(out,prefix+"legacy-origin-missing",type,"items",labels,null,null,actual);
   add(out,prefix+"origin-length-mismatch",type,"items",labels,source,List.of("wrong"),actual);
   add(out,prefix+"other-table",type,"items",labels,source,List.of("other","other"),actual);
   add(out,prefix+"blank-source",type,"items",labels,List.of("","name"),tables,actual);
   add(out,prefix+"null-source",type,"items",labels,Arrays.asList(null,"name"),Arrays.asList(null,"items"),actual);
   add(out,prefix+"missing-table",type,"items",labels,source,tables,null);
   add(out,prefix+"empty-result",type,"items",null,null,null,actual);
  }
  for(var pair:new String[][]{{"I","ı"},{"i","İ"},{"S","ſ"},{"ß","ẞ"},{"ß","SS"},{"Σ","ς"},{"σ","Σ"},{"𐐀","𐐨"},{"é","É"},{"ﬀ","FF"},{"µ","Μ"},{"k","K"}}){add(out,"unicode-"+pair[0]+"-"+pair[1],ConnectType.POSTGRESQL,"items",List.of(pair[1]),List.of(pair[1]),List.of("items"),columns(new String[]{pair[0]},pair[0]));}
  add(out,"ambiguous-case",ConnectType.POSTGRESQL,"items",List.of("ID"),null,null,columns(new String[]{"Id","id"},"Id"));
  add(out,"exact-case-wins",ConnectType.POSTGRESQL,"items",List.of("id"),null,null,columns(new String[]{"Id","id"},"id"));
  var blanks=new ArrayList<Map<String,String>>();blanks.add(null);blanks.add(Map.of());blanks.add(Map.of("字段名","\u2003","主键","是"));blanks.add(Map.of("字段名","\u00a0","主键","是"));
  add(out,"java-blank",ConnectType.MYSQL,"items",List.of("\u00a0"),List.of("\u00a0"),List.of("items"),blanks);
  var output=Path.of(args[0]);Files.createDirectories(output.getParent());Files.writeString(output,new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create().toJson(out));
 }
}
