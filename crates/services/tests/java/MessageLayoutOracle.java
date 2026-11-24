import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.view.*;

/** Actual original MQ metadata widgets and native tree labels, owned loopback only. */
public class MessageLayoutOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var args=getParameters().getRaw();var out=Path.of(args.get(0));Files.createDirectories(out);
   if(!System.getProperty("user.home").contains("oracle-message-layout-"))throw new IllegalStateException("Isolated home required");
   // Constructors fetch topics asynchronously. Their only destination is this
   // process's own loopback listener; no saved hosts or configuration are read.
   var listener=new java.net.ServerSocket(0,1,java.net.InetAddress.getLoopbackAddress());
   var config=new ConnectionConfig();config.setId("owned-layout");config.setName("布局验收");config.setType(ConnectType.valueOf(args.get(1)));config.setHost("127.0.0.1");config.setPort(listener.getLocalPort());
   Object view=args.get(1).equals("KAFKA")?new KafkaDataView(config,""):new RocketmqDataView(config,"");
   var details=new TreeMap<String,Object>();
   details.put("topic","orders.production");details.put("msgId","7F00000100002A9F00000000000004D2");details.put("offset",1234567890L);details.put("queueOffset",1234567890L);details.put("partition",2);details.put("key","customer-order-20261020-1234567890");details.put("timestamp",1792454400000L);
   details.put("headers",List.of(Map.of("key","trace-id","value","0123456789abcdef0123456789abcdef"),Map.of("key","来源","value","生产环境订单服务")));
   var fill=view.getClass().getDeclaredMethod("fillMessageDetailPanel",Map.class);fill.setAccessible(true);fill.invoke(view,details);
   var field=view.getClass().getDeclaredField("messageInfoGrid");field.setAccessible(true);var grid=(GridPane)field.get(view);((Pane)grid.getParent()).getChildren().remove(grid);
   var rootItem=new TreeItem<String>("root");for(var label:List.of("本地终端","SSH 生产服务器","MySQL 数据库","本地目录"))rootItem.getChildren().add(new TreeItem<String>(label));rootItem.setExpanded(true);
   var tree=new TreeView<String>(rootItem);tree.setShowRoot(false);tree.setFixedCellSize(35);tree.setStyle("-fx-background-color: transparent; -fx-cell-size: 35px;");tree.setMinWidth(240);tree.setPrefWidth(240);tree.setMaxWidth(240);
   tree.getStylesheets().add(Path.of(args.get(3),"css/connect-tree.css").toUri().toString());
   var root=new HBox(tree,new VBox(grid));stage.setScene(new Scene(root,240+Integer.parseInt(args.get(2)),380));stage.setOpacity(.01);stage.show();
   var pause=new PauseTransition(Duration.millis(500));pause.setOnFinished(event->{try{
    root.applyCss();root.layout();var path=out.resolve(args.get(1).toLowerCase()+"-"+args.get(2)+".png");ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
    var nodes=new JsonArray();walk(root,nodes);var proof=new JsonObject();proof.add("labels",nodes);proof.addProperty("originalMetadataMethodInvoked",true);proof.addProperty("onlyNetworkDestination","127.0.0.1:"+listener.getLocalPort());Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(proof));listener.close();stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});pause.play();
  }
 }
 static void walk(Node node,JsonArray nodes){if(node instanceof Labeled label&&label.getText()!=null&&!label.getText().isEmpty()){
  var item=new JsonObject();var b=node.localToScene(node.getLayoutBounds());item.addProperty("text",label.getText());item.addProperty("font",label.getFont().toString());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());item.addProperty("baseline",label.getBaselineOffset());nodes.add(item);
 }if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,nodes);}
}
