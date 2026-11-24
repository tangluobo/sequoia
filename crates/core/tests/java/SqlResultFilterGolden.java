package com.tangluobo.tomato.module.connect.view;
import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
/** Invokes only pure original filter methods. No UI, servers or clipboard. */
public class SqlResultFilterGolden {
 @SuppressWarnings({"unchecked","rawtypes"})
 public static void main(String[] args)throws Exception {
  var utility=TableCellContextMenuUtils.class;
  var operator=Class.forName(utility.getName()+"$FilterOperator");var condition=Class.forName(utility.getName()+"$FilterCondition");
  var constructor=condition.getDeclaredConstructor(operator,String.class);constructor.setAccessible(true);var matches=condition.getDeclaredMethod("matches",String.class);matches.setAccessible(true);
  var compare=utility.getDeclaredMethod("compareValues",String.class,String.class);compare.setAccessible(true);var parse=utility.getDeclaredMethod("parseFilter",String.class);parse.setAccessible(true);
  String[] values={null,"","0","-0.00","1","2","10","2.000","-2","+1.0e-3","1E+3","0.0000001","000.100","NaN"," 2 ","\u20032\u2003","\u00a02","1e2147483647","1e2147483648","1e-2147483647","1e-2147483648","١٢.٣","१२.३","１２.３E４","😀","\ue000","𐐀","𐐨","I","i","İ","ı","ſ","S","ß","ẞ","SS","Σ","ς","ΟΣ","中文","中文\n","résumé","Resume"};
  var output=new JsonObject();var cases=new JsonArray();var gson=new GsonBuilder().serializeNulls().create();
  for(var actual:values)for(var expected:values){var item=new JsonObject();item.add("actual",gson.toJsonTree(actual));item.add("expected",gson.toJsonTree(expected));item.addProperty("comparison",Integer.signum((Integer)compare.invoke(null,actual,expected)));var results=new JsonObject();for(var name:new String[]{"EQUAL","NOT_EQUAL","LIKE","NOT_LIKE","LESS","GREATER"}){var value=Enum.valueOf((Class)operator,name);var filter=constructor.newInstance(value,expected);results.addProperty(name,(Boolean)matches.invoke(filter,actual));}item.add("matches",results);cases.add(item);}output.add("cases",cases);
  var parsed=new JsonArray();for(var input:new String[]{null,""," ","text","= value","!= value","~ value","!~ value","< 12","> 2","<= 2",">= 2","==x","!~","\u2003!=\u2003值\u2003","\u00a0= x","\u001c=\u001d值\u001e","\u0000=x"}){var value=parse.invoke(null,input);var op=value.getClass().getDeclaredField("operator");var text=value.getClass().getDeclaredField("value");op.setAccessible(true);text.setAccessible(true);var item=new JsonObject();item.add("input",gson.toJsonTree(input));item.addProperty("operator",op.get(value).toString());item.addProperty("value",(String)text.get(value));parsed.add(item);}output.add("parsed",parsed);
  var digits=new JsonArray();for(int start=0;start<=0xffff-9;start++){boolean zero=true;for(int offset=0;offset<10;offset++)if(Character.digit((char)(start+offset),10)!=offset){zero=false;break;}if(zero)digits.add(start);}output.add("digitZeroes",digits);
  var path=Path.of(args[0]);Files.createDirectories(path.getParent());Files.writeString(path,new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create().toJson(output));
 }
}
