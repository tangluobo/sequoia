import javafx.application.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
import java.util.*;
/** Reads the actual original information TextArea, without servers or clipboard. */
public class SqlInformationGolden {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   Locale.setDefault(Locale.SIMPLIFIED_CHINESE);var view=new SqlEditorView(List.of(),null,null);var method=SqlEditorView.class.getDeclaredMethod("buildInfoTab",MultiStatementResult.class);method.setAccessible(true);
   var cases=new JsonArray();
   for(int sample=0;sample<5;sample++){
    var statements=new ArrayList<SqlStatementResult>();var inputs=new JsonArray();
    int count=sample==0?0:sample==1?1:sample==2?3:5;
    for(int i=0;i<count;i++){
     var statement=new SqlStatementResult();var input=new JsonObject();var sql=(i%2==0?"SELECT '中文;换行\\n' FROM items":"UPDATE items SET value='a''b'")+(sample==4?" /* "+"long note ".repeat(40)+" */":"");
     boolean success=sample<3||i!=1;boolean rowset=i%2==0;long time=new long[]{0,1,999,1001,1234567890}[i];long rows=i==4?1234567890:7;int affected=i==3?-1:3;
     statement.setSql(sql);statement.setSuccess(success);statement.setQueryTime(time);statement.setHasResultSet(rowset);statement.setUpdateCount(affected);statement.setErrorMessage("错误行一\n错误行二");
     if(rowset){var data=new TableRowData();data.setTotalCount(rows);statement.setResultData(data);}statements.add(statement);
     input.addProperty("sql",sql);input.addProperty("success",success);input.addProperty("rowset",rowset);input.addProperty("time",time);input.addProperty("rows",rows);input.addProperty("affected",affected);input.addProperty("error","错误行一\n错误行二");inputs.add(input);
    }
    var multi=new MultiStatementResult();multi.setResults(statements);multi.setTotalTime(2345);var tab=(Tab)method.invoke(view,multi);var area=(TextArea)tab.getContent();var item=new JsonObject();item.add("statements",inputs);item.addProperty("time",2345);item.addProperty("text",area.getText());item.addProperty("wrap",area.isWrapText());item.addProperty("editable",area.isEditable());cases.add(item);
   }
   var path=Path.of(getParameters().getRaw().get(0));Files.createDirectories(path.getParent());Files.writeString(path,new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(cases));Platform.exit();System.exit(0);
  }
 }
}
