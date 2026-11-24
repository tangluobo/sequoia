import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.dialog.*;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import java.nio.file.*;
import java.lang.reflect.*;
import java.sql.*;
import java.util.*;
import javax.imageio.ImageIO;
/** Unchanged dialog controls, isolated synthetic JDBC and Scene snapshots only. */
public class DatabaseAdminWindowOracle {
 static java.sql.Connection connection(){return DatabaseObjectsWindowOracle.proxy(java.sql.Connection.class,(self,method,args)->switch(method.getName()){
 case "isValid"->true;
 case "createStatement"->DatabaseObjectsWindowOracle.proxy(Statement.class,(s,m,a)->m.getName().equals("executeQuery")?DatabaseObjectsWindowOracle.result(List.of(DatabaseObjectsWindowOracle.row("CHARACTER_SET_NAME","utf8mb4","COLLATION_NAME","utf8mb4_0900_ai_ci"),DatabaseObjectsWindowOracle.row("CHARACTER_SET_NAME","utf8mb4","COLLATION_NAME","utf8mb4_general_ci"))):DatabaseObjectsWindowOracle.basic(m));
 default->DatabaseObjectsWindowOracle.basic(method);
 });}
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application{
 @SuppressWarnings({"unchecked","rawtypes"}) public void start(Stage owner)throws Exception{
 var c=new ConnectionConfig();c.setId("admin-oracle-only");c.setType(ConnectType.MYSQL);c.setHost("127.0.0.1");c.setPort(3306);c.setDatabase("sample_db");
 var cache=DatabaseService.class.getDeclaredField("connectionCache");cache.setAccessible(true);((Map)cache.get(null)).put(c.getId(),connection());
 boolean create=getParameters().getRaw().get(1).equals("create");Object dialog=create?new CreateDatabaseDialog(owner,c):new EditDatabaseDialog(owner,c,"sample_db","utf8mb4","utf8mb4_0900_ai_ci");
 var f=dialog.getClass().getDeclaredField("dialogStage");f.setAccessible(true);var original=(Stage)f.get(dialog);var scene=original.getScene();original.setScene(null);owner.setScene(scene);owner.setOpacity(.01);owner.show();
 var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
 if(create){var name=dialog.getClass().getDeclaredField("nameField");name.setAccessible(true);((TextField)name.get(dialog)).setText("sample_db");}
 if(getParameters().getRaw().get(2).equals("1")){((TabPane)scene.getRoot().lookup(".tab-pane")).getSelectionModel().select(1);}
 scene.getRoot().applyCss();scene.getRoot().layout();
 var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(scene.getRoot().snapshot(null,null),null),"png",path.toFile());var out=new JsonObject();out.addProperty("width",scene.getWidth());out.addProperty("height",scene.getHeight());var q=dialog.getClass().getDeclaredField("currentSql");q.setAccessible(true);out.addProperty("sql",(String)q.get(dialog));out.addProperty("syntheticJdbcOnly",true);out.addProperty("socketsOpened",false);var controls=new JsonArray();DatabaseObjectsWindowOracle.walk(scene.getRoot(),controls);out.add("controls",controls);Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(out));owner.close();Platform.exit();System.exit(0);
 }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
 }
 }
}
